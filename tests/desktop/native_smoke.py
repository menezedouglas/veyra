#!/usr/bin/env python3
"""Exercise the installed Rust/Tauri/WebKit application with real native IPC.

No Selenium dependency, command mock, remote endpoint, user message, or private
key is used. Responses stay in this Python process; output contains only check
names and filesystem paths. Run inside an isolated Linux desktop/test container.
Requires tauri-driver 2.1.0, WebKitWebDriver, Xvfb, and the two compiled binaries.
"""
from pathlib import Path
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.error import HTTPError, URLError
import argparse
import json
import os
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


class Driver:
    def __init__(self, port):
        self.base = f'http://127.0.0.1:{port}'
        self.session = None
        self.last_error_message = ''

    def request(self, method, path, body=None, timeout=35):
        data = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(self.base + path, data=data, method=method,
                                     headers={'Content-Type': 'application/json'})
        try:
            with urllib.request.urlopen(req, timeout=timeout) as response:
                result = json.load(response)
        except HTTPError as error:
            # Never print driver responses: those can contain DOM/component data.
            result = json.loads(error.read())
            value = result.get('value', {})
            self.last_error_message = value.get('message', '')
            raise RuntimeError(value.get('error', 'WebDriver request failed')) from None
        value = result.get('value')
        if isinstance(value, dict) and value.get('error'):
            raise RuntimeError(value['error'])
        return value

    def call(self, endpoint, body=None, method='POST'):
        return self.request(method, f'/session/{self.session}/{endpoint}', body)

    def script(self, script, args=None):
        return self.call('execute/sync', {'script': script, 'args': args or []})

    def invoke(self, command, args=None):
        return self.call('execute/async', {
            'script': "const done=arguments[arguments.length-1];"
                      "if(!window.__TAURI_INTERNALS__){done({bridge:false,accepted:false});return;}"
                      "window.__TAURI_INTERNALS__.invoke(arguments[0],arguments[1])"
                      ".then(value=>done({bridge:true,accepted:true,value}))"
                      ".catch(()=>done({bridge:true,accepted:false}));",
            'args': [command, args or {}],
        })

    def status(self):
        response = self.invoke('connection_status')
        assert response['accepted'], 'native status command was rejected'
        return response['value']

    def click(self, text):
        element = self.call('element', {'using': 'xpath', 'value': f"//button[contains(.,'{text}')]"})
        identifier = element['element-6066-11e4-a52e-4f735466cecf']
        self.call(f'element/{identifier}/click', {})


def wait_for(check, seconds=20):
    end = time.monotonic() + seconds
    last = None
    while time.monotonic() < end:
        try:
            last = check()
            if last:
                return last
        except (RuntimeError, AssertionError, URLError, ConnectionError, TimeoutError):
            pass
        time.sleep(.15)
    raise AssertionError('condition did not become true before its fixed deadline')


def proc_ids(binary):
    found = set()
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            if entry.joinpath('exe').resolve() == binary:
                found.add(int(entry.name))
        except (OSError, RuntimeError):
            pass
    return found


def children_named(parent, name):
    found = set()
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            values = dict(line.split(':', 1) for line in entry.joinpath('status').read_text().splitlines() if ':' in line)
            if int(values.get('PPid', '-1')) == parent and values.get('Name', '').strip() == name:
                found.add(int(entry.name))
        except (OSError, ValueError):
            pass
    return found


class LocalPage(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b'<!doctype html><title>Veyra synthetic origin test</title><p>Local test fixture</p>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


def inspect_files(root, needles):
    files = []
    hits = 0
    scanned = 0
    for path in sorted(root.rglob('*')):
        if path.is_file() and not path.is_symlink():
            size = path.stat().st_size
            files.append({'path': str(path.relative_to(root)), 'bytes': size})
            if size <= 16 * 1024 * 1024:
                data = path.read_bytes()
                scanned += 1
                if any(needle in data for needle in needles):
                    hits += 1
    return {'files': files, 'scannedFiles': scanned, 'identityMetadataMatches': hits}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--driver', default='tauri-driver')
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    assert binary.is_file(), 'desktop executable missing'
    assert binary.with_name('veyra-vault').is_file(), 'vault executable missing'
    port, native_port, display_number = free_port(), free_port(), 121
    checks = {}
    logs = bytearray()
    processes = []
    session = None
    http_server = None
    before = proc_ids(binary)
    needles = []
    with tempfile.TemporaryDirectory(prefix='veyra-desktop-qa-') as directory:
        root = Path(directory)
        paths = {name: root / name for name in ('config', 'data', 'cache', 'runtime', 'tmp')}
        for path in paths.values():
            path.mkdir(mode=0o700)
        env = os.environ.copy()
        env.update({
            'DISPLAY': f':{display_number}',
            'XDG_CONFIG_HOME': str(paths['config']),
            'XDG_DATA_HOME': str(paths['data']),
            'XDG_CACHE_HOME': str(paths['cache']),
            'XDG_RUNTIME_DIR': str(paths['runtime']),
            'TMPDIR': str(paths['tmp']),
            'WEBKIT_DISABLE_DMABUF_RENDERER': '1',
            'LIBGL_ALWAYS_SOFTWARE': '1',
            'NO_AT_BRIDGE': '1',
        })
        def spawn(command):
            process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                       start_new_session=True)
            processes.append(process)
            def read_output():
                # Bounded RAM-only diagnostic capture, never printed verbatim.
                for block in iter(lambda: process.stdout.read(4096), b''):
                    if len(logs) < 1024 * 1024:
                        logs.extend(block[:1024 * 1024-len(logs)])
            threading.Thread(target=read_output, daemon=True).start()
            return process
        driver = Driver(port)
        try:
            spawn(['Xvfb', f':{display_number}', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'])
            wait_for(lambda: Path(f'/tmp/.X11-unix/X{display_number}').exists(), seconds=10)
            spawn([args.driver, '--port', str(port), '--native-port', str(native_port)])
            wait_for(lambda: driver.request('GET', '/status'), seconds=10)
            try:
                value = driver.request('POST', '/session', {'capabilities': {'alwaysMatch': {
                    'tauri:options': {'application': str(binary)},
                }}})
            except RuntimeError:
                # No session or identity has been created at this point.
                print('Session creation diagnostic:', driver.last_error_message[:1500])
                raise
            session = value['sessionId']
            driver.session = session
            driver.call('timeouts', {'script': 5000, 'implicit': 0, 'pageLoad': 20000})
            wait_for(lambda: driver.script("return document.title==='Veyra' && !!document.querySelector('button')"))
            checks['native_window_loaded'] = True
            assert driver.status()['phase'] == 'inactive'
            checks['session_initially_inactive'] = True
            driver.click('Iniciar sessão')
            status = wait_for(lambda: (s if (s := driver.status())['phase'] == 'active' else None))
            assert status['runtimeId'] and status['fingerprint']
            first_runtime = status['runtimeId']
            first_fingerprint = status['fingerprint']
            needles.extend([first_runtime.encode(), first_fingerprint.encode()])
            checks['native_session_generated_identity'] = True
            assert status['memoryProtection'] == 'protected'
            checks['native_memory_guard_confirmed'] = True
            application_pids = wait_for(lambda: proc_ids(binary) - before)
            assert len(application_pids) == 1
            application_pid = next(iter(application_pids))
            vault_pids = wait_for(lambda: children_named(application_pid, 'veyra-vault'))
            checks['separate_vault_process_observed'] = bool(vault_pids)
            pending = driver.invoke('list_pending')
            assert pending['accepted'] and pending['value'] == []
            checks['pending_metadata_has_no_plaintext'] = True
            for name, command, values in [
                ('background_not_silently_enabled', 'start_session', {'allowBackground': True}),
                ('unconfirmed_open_rejected', 'open_once', {'messageId': '0' * 32, 'confirmed': False}),
                ('unconfigured_network_rejected', 'create_invite', {}),
                ('shell_command_rejected', 'plugin:shell|execute', {'program': 'true'}),
                ('filesystem_command_rejected', 'plugin:fs|read_text_file', {'path': '/etc/passwd'}),
                ('window_creation_rejected', 'plugin:webview|create_webview_window', {'options': {'label': 'untrusted'}}),
            ]:
                assert not driver.invoke(command, values)['accepted'], name
                checks[name] = True
            driver.call('refresh', {})
            wait_for(lambda: driver.script("return !!document.querySelector('button')"))
            assert driver.status()['runtimeId'] == first_runtime
            checks['webview_reload_preserves_core_identity'] = True
            driver.click('Sair e descartar sessão')
            ended = wait_for(lambda: (s if (s := driver.status())['phase'] == 'inactive' else None))
            assert not ended['runtimeId'] and not ended['fingerprint']
            wait_for(lambda: not children_named(application_pid, 'veyra-vault'))
            checks['end_session_clears_identity_and_stops_vault'] = True
            assert not driver.script('return document.body.textContent.includes(arguments[0])', [first_fingerprint])
            checks['ended_identity_removed_from_dom'] = True
            driver.click('Iniciar sessão')
            restarted = wait_for(lambda: (s if (s := driver.status())['phase'] == 'active' else None))
            assert restarted['runtimeId'] != first_runtime and restarted['fingerprint'] != first_fingerprint
            needles.extend([restarted['runtimeId'].encode(), restarted['fingerprint'].encode()])
            checks['new_session_changes_identity'] = True
            driver.click('Sair e descartar sessão')
            wait_for(lambda: driver.status()['phase'] == 'inactive')
            http_server = HTTPServer(('127.0.0.1', 0), LocalPage)
            threading.Thread(target=http_server.serve_forever, daemon=True).start()
            remote_url = f'http://127.0.0.1:{http_server.server_port}/'
            try:
                driver.call('url', {'url': remote_url})
                if driver.script('return location.origin') != remote_url.rstrip('/'):
                    checks['external_origin_navigation_blocked'] = True
                else:
                    response = driver.invoke('connection_status')
                    assert not response['accepted']
                    checks['external_origin_ipc_rejected'] = True
            except RuntimeError as error:
                # A timeout is reported separately: it proves no successful
                # response was observed, not that the Rust guard ran.
                if str(error) == 'script timeout':
                    checks['external_origin_ipc_timed_out_without_authorized_response'] = True
                else:
                    raise
            checks['identity_metadata_absent_from_captured_process_output'] = not any(n in logs for n in needles)
            assert checks['identity_metadata_absent_from_captured_process_output']
        finally:
            if session:
                try:
                    driver.request('DELETE', f'/session/{session}')
                except Exception:
                    pass
            if http_server:
                http_server.shutdown()
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
        inspection = inspect_files(root, needles)
        assert inspection['identityMetadataMatches'] == 0, 'identity metadata appeared in an inspected file'
        checks['identity_metadata_absent_from_inspected_xdg_files'] = True
        report = {'checks': checks, 'filesystem': inspection, 'capturedProcessOutputBytes': len(logs),
                  'limitations': ['No user message flow is implemented in this desktop increment.',
                                  'Only isolated XDG/TMP paths and bounded stdout/stderr were inspected.',
                                  'No proof of absence of OS, GPU, swap, or privileged-process traces.']}
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'passedChecks': len(checks), 'checks': checks,
                          'inspectedFiles': len(inspection['files'])}, indent=2))


if __name__ == '__main__':
    main()
