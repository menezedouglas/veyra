#!/usr/bin/env python3
"""Drive the real Linux Tauri UI over AT-SPI, preserving incognito=true.

This test uses no IPC mocks, messages, network endpoint, user data, or private-key
access. Public test identity metadata stays in RAM and is used only as a scan
needle. Requires python3-pyatspi, Xvfb, D-Bus, xdotool and the compiled binaries.
"""
from pathlib import Path
import argparse
import json
import os
import re
import select
import signal
import subprocess
import tempfile
import threading
import time


class CheckFailure(Exception):
    pass


def wait_for(check, name, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (RuntimeError, LookupError, OSError):
            pass
        time.sleep(.15)
    raise CheckFailure(name)


def child_vaults(parent):
    live, zombies = set(), set()
    for directory in Path('/proc').iterdir():
        if not directory.name.isdecimal():
            continue
        try:
            values = dict(line.split(':', 1) for line in (directory/'status').read_text().splitlines() if ':' in line)
            if int(values.get('PPid', '-1')) == parent and values.get('Name', '').strip() == 'veyra-vault':
                (zombies if values.get('State', '').strip().startswith('Z') else live).add(int(directory.name))
        except (ValueError, OSError):
            pass
    return live, zombies


def inspect_files(root, needles):
    files, matches = [], 0
    for path in sorted(root.rglob('*')):
        if path.is_file() and not path.is_symlink():
            size = path.stat().st_size
            files.append({'path': str(path.relative_to(root)), 'bytes': size})
            if size <= 16 * 1024 * 1024 and any(needle in path.read_bytes() for needle in needles):
                matches += 1
    return {'files': files, 'identityMetadataMatches': matches,
            'scanLimitPerFileBytes': 16 * 1024 * 1024}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--screenshot', type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file() or not binary.with_name('veyra-vault').is_file():
        raise SystemExit('Required desktop/vault binaries are missing.')
    checks, needles, logs, processes = {}, [], bytearray(), []
    failure = None
    screenshot_saved = False
    with tempfile.TemporaryDirectory(prefix='veyra-accessibility-qa-') as directory:
        fixture = Path(directory)
        env = os.environ.copy()
        for name, variable in [('config', 'XDG_CONFIG_HOME'), ('data', 'XDG_DATA_HOME'), ('cache', 'XDG_CACHE_HOME'), ('runtime', 'XDG_RUNTIME_DIR'), ('tmp', 'TMPDIR')]:
            path = fixture/name
            path.mkdir(mode=0o700)
            env[variable] = str(path)
        display_number = next(n for n in range(122, 142) if not Path(f'/tmp/.X11-unix/X{n}').exists())
        env.update({'DISPLAY': f':{display_number}', 'GTK_MODULES': 'atk-bridge',
                    'GSETTINGS_BACKEND': 'memory', 'WEBKIT_DISABLE_DMABUF_RENDERER': '1',
                    'LIBGL_ALWAYS_SOFTWARE': '1'})
        env.pop('NO_AT_BRIDGE', None)
        env.pop('TAURI_WEBVIEW_AUTOMATION', None)

        def read_logs(process):
            for block in iter(lambda: process.stdout.read(4096), b''):
                if len(logs) < 1024 * 1024:
                    logs.extend(block[:1024 * 1024-len(logs)])

        def spawn(command, read=True):
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            processes.append(process)
            if read:
                threading.Thread(target=read_logs, args=(process,), daemon=True).start()
            return process

        def screenshot():
            nonlocal screenshot_saved
            if not args.screenshot:
                return
            args.screenshot.parent.mkdir(parents=True, exist_ok=True)
            # Capture only before identity generation or after confirmed end.
            result = subprocess.run(['xwd', '-silent', '-root', '-out', str(args.screenshot)],
                                    env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
            screenshot_saved = result.returncode == 0

        try:
            spawn(['Xvfb', env['DISPLAY'], '-screen', '0', '1280x900x24', '-nolisten', 'tcp'])
            wait_for(lambda: Path(f'/tmp/.X11-unix/X{display_number}').exists(), 'xvfb_startup', seconds=10)
            bus = spawn(['dbus-daemon', '--session', '--nofork', '--print-address=1', '--nopidfile'], read=False)
            if not select.select([bus.stdout], [], [], 5)[0]:
                raise CheckFailure('dbus_startup')
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().decode().strip()
            threading.Thread(target=read_logs, args=(bus,), daemon=True).start()
            # AT-SPI must initialize after this process joins the private D-Bus.
            os.environ.update({key: value for key, value in env.items() if key in (
                'DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR', 'GSETTINGS_BACKEND')})
            import pyatspi
            from gi.repository import Gio, GLib
            try:
                proxy = Gio.DBusProxy.new_for_bus_sync(Gio.BusType.SESSION, Gio.DBusProxyFlags.NONE,
                    None, 'org.a11y.Bus', '/org/a11y/bus', 'org.freedesktop.DBus.Properties', None)
                for name in ('IsEnabled', 'ScreenReaderEnabled'):
                    proxy.call_sync('Set', GLib.Variant('(ssv)', ('org.a11y.Status', name, GLib.Variant('b', True))),
                                    Gio.DBusCallFlags.NONE, 5000, None)
            except Exception:
                # A running AT-SPI bus may already have these settings enabled.
                pass
            application = spawn([str(binary)])
            window_ids = wait_for(lambda: subprocess.run(['xdotool', 'search', '--name', '^Veyra$'],
                env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5).stdout.strip(),
                'native_window_visible')
            checks['native_window_visible'] = bool(window_ids)
            screenshot()

            def nodes():
                desktop = pyatspi.Registry.getDesktop(0)
                queue = [(desktop, 0)]
                seen = 0
                while queue and seen < 2048:
                    node, depth = queue.pop(0)
                    seen += 1
                    yield node
                    if depth >= 24:
                        continue
                    try:
                        queue.extend((node[index], depth+1) for index in range(min(node.childCount, 512)))
                    except Exception:
                        pass

            def all_text():
                values = []
                for node in nodes():
                    try:
                        if node.name:
                            values.append(node.name[:8192])
                        try:
                            text = node.queryText().getText(0, -1)
                            if text:
                                values.append(text[:8192])
                        except (NotImplementedError, AttributeError):
                            pass
                    except Exception:
                        pass
                return '\n'.join(values)

            def button(name):
                for node in nodes():
                    try:
                        if node.getRole() == pyatspi.ROLE_PUSH_BUTTON and node.name.strip() == name:
                            return node
                    except Exception:
                        pass
                return None

            def press(name):
                node = wait_for(lambda: button(name), 'button_available_' + name, seconds=10)
                try:
                    action = node.queryAction()
                    if not action.nActions or not action.doAction(0):
                        raise CheckFailure('button_action_' + name)
                except Exception:
                    raise CheckFailure('button_action_' + name) from None

            wait_for(lambda: button('Iniciar sessão'), 'accessible_start_button', seconds=25)
            checks['atspi_real_document_available'] = True
            for label in ('Mensagens pendentes', 'Contatos da sessão', 'Convites', 'Conexões', 'Configurações'):
                if not button(label):
                    raise CheckFailure('navigation_' + label)
            checks['five_navigation_tabs_accessible'] = True
            if 'Sessão não iniciada' not in all_text():
                raise CheckFailure('initial_session_inactive')
            checks['initial_session_inactive'] = True
            if child_vaults(application.pid)[0]:
                raise CheckFailure('vault_running_before_start')
            press('Iniciar sessão')
            live = wait_for(lambda: child_vaults(application.pid)[0], 'vault_process_started_by_real_ui')
            checks['vault_process_started_by_real_ui'] = bool(live)
            wait_for(lambda: 'Identidade ativa nesta execução' in all_text(), 'active_identity_ui')
            fingerprint_pattern = r'\b[0-9A-F]{4}(?: [0-9A-F]{4}){15}\b'
            identity = wait_for(lambda: re.search(fingerprint_pattern, all_text()), 'public_fingerprint_displayed').group(0)
            needles.extend([identity.encode(), identity.replace(' ', '').encode()])
            checks['public_fingerprint_displayed'] = True
            wait_for(lambda: 'Nenhuma mensagem pendente' in all_text(), 'empty_pending_without_mock')
            checks['empty_pending_without_mock'] = True
            press('Conexões')
            text = wait_for(lambda: (value if 'Nenhum transporte estabelecido.' in (value := all_text()) else None), 'unconfigured_transport_truthful')
            if 'P2P direto' in text or 'via relay' in text:
                raise CheckFailure('invented_route_detected')
            checks['unconfigured_transport_truthful'] = True
            checks['no_invented_direct_or_relay_route'] = True
            press('Configurações')
            text = wait_for(lambda: (value if 'Identidade desta execução' in (value := all_text()) else None), 'identity_settings')
            runtime = re.search(r'\b[0-9a-f]{32}\b', text)
            if not runtime:
                raise CheckFailure('runtime_id_displayed')
            needles.append(runtime.group(0).encode())
            checks['runtime_id_displayed'] = True
            press('Sair e descartar sessão')
            wait_for(lambda: not child_vaults(application.pid)[0], 'vault_terminated_after_end')
            checks['vault_terminated_after_end'] = True
            text = wait_for(lambda: (value if 'Sessão não iniciada' in (value := all_text()) else None), 'session_ended_ui')
            if identity in text or runtime.group(0) in text:
                raise CheckFailure('ended_identity_remained_in_accessible_ui')
            checks['ended_identity_removed_from_accessible_ui'] = True
            checks['no_live_vault_after_end'] = not child_vaults(application.pid)[0]
            checks['no_zombie_vault_observed_after_end'] = not child_vaults(application.pid)[1]
            screenshot()
            if any(needle in logs for needle in needles):
                raise CheckFailure('public_identity_metadata_in_process_output')
            checks['identity_metadata_absent_from_captured_process_output'] = True
        except CheckFailure as error:
            failure = str(error)
        except Exception as error:
            # Report only the exception class; a DBus/DOM error can embed text.
            failure = type(error).__name__
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
            for process in processes:
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
        inspection = inspect_files(fixture, needles)
        if inspection['identityMetadataMatches']:
            failure = failure or 'public_identity_metadata_in_inspected_file'
        elif needles:
            checks['identity_metadata_absent_from_inspected_xdg_files'] = True
        report = {'checks': checks, 'failure': failure, 'filesystem': inspection,
                  'capturedOutputBytes': len(logs), 'screenshotSaved': screenshot_saved,
                  'limitations': ['No message, invitation, pairing or remote endpoint was used.',
                                  'Only the isolated XDG/TMP directories and bounded process output were inspected.',
                                  'No conclusion about all OS, GPU, swap, crash or privileged-process traces.']}
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'passedChecks': len(checks), 'checks': checks, 'failure': failure,
                          'inspectedFiles': len(inspection['files']), 'screenshotSaved': screenshot_saved}, indent=2))
        if failure:
            raise SystemExit(1)


if __name__ == '__main__':
    main()
