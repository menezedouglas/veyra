#!/usr/bin/env python3
"""Two genuine Tauri windows, native workers, WSS, Noise/HPKE and one-time UI.

The operator CA/TLS files are synthetic operational configuration. User identity,
invites and message fixtures remain in RAM. X11 typing receives values over stdin,
never command arguments. No screenshots, mocks, external operator or WebDriver.
"""
from pathlib import Path
import argparse
import json
import os
import re
import select
import signal
import socket
import subprocess
import tempfile
import threading
import time
from native_accessibility import CheckFailure, wait_for, child_vaults, inspect_files


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--signaling', required=True, type=Path)
    parser.add_argument('--port', type=int, default=19443)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    binary, signaling = args.binary.resolve(), args.signaling.resolve()
    if not binary.is_file() or not signaling.is_file():
        raise SystemExit('Required compiled executables are missing.')
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', args.port))
    checks, needles, processes, logs = {}, [], [], bytearray()
    failure = None
    windows = []
    failure_state = []
    with tempfile.TemporaryDirectory(prefix='veyra-two-desktops-') as directory:
        fixture = Path(directory)
        operator = fixture/'operator'; operator.mkdir(mode=0o700)
        bus_home = fixture/'bus'; bus_home.mkdir(mode=0o700)
        env = os.environ.copy()
        display_number = next(n for n in range(142, 162) if not Path(f'/tmp/.X11-unix/X{n}').exists())
        env.update({'DISPLAY': f':{display_number}', 'GTK_MODULES': 'atk-bridge',
                    'GSETTINGS_BACKEND': 'memory', 'WEBKIT_DISABLE_DMABUF_RENDERER': '1',
                    'LIBGL_ALWAYS_SOFTWARE': '1', 'XDG_RUNTIME_DIR': str(bus_home)})
        env.pop('NO_AT_BRIDGE', None)
        env.pop('TAURI_WEBVIEW_AUTOMATION', None)

        def quiet(command):
            subprocess.run(command, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        quiet(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(operator/'ca.key'),
               '-out', str(operator/'ca.pem'), '-days', '1', '-subj', '/CN=Veyra synthetic desktop QA CA',
               '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign'])
        quiet(['openssl', 'req', '-new', '-newkey', 'rsa:2048', '-nodes', '-keyout', str(operator/'server.key'),
               '-out', str(operator/'server.csr'), '-subj', '/CN=localhost'])
        (operator/'server.ext').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n')
        quiet(['openssl', 'x509', '-req', '-in', str(operator/'server.csr'), '-CA', str(operator/'ca.pem'),
               '-CAkey', str(operator/'ca.key'), '-CAcreateserial', '-out', str(operator/'server.pem'),
               '-days', '1', '-extfile', str(operator/'server.ext')])
        env['SSL_CERT_FILE'] = str(operator/'ca.pem')
        env['VEYRA_SIGNALING_ENDPOINT'] = f'wss://localhost:{args.port}/v1/rendezvous'

        def read_logs(process):
            for block in iter(lambda: process.stdout.read(4096), b''):
                if len(logs) < 2 * 1024 * 1024:
                    logs.extend(block[:2 * 1024 * 1024-len(logs)])
        def spawn(command, process_env=None, read=True):
            process = subprocess.Popen(command, env=process_env or env, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, start_new_session=True)
            processes.append(process)
            if read:
                threading.Thread(target=read_logs, args=(process,), daemon=True).start()
            return process
        def xdo(*arguments, text=None):
            result = subprocess.run(['xdotool', *arguments], env=env,
                input=text.encode() if text is not None else None, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, timeout=10)
            if result.returncode:
                raise CheckFailure('x11_' + arguments[0] + '_exit_' + str(result.returncode))
            return result.stdout

        try:
            spawn(['Xvfb', env['DISPLAY'], '-screen', '0', '1280x900x24', '-nolisten', 'tcp'])
            wait_for(lambda: Path(f'/tmp/.X11-unix/X{display_number}').exists(), 'xvfb_startup', 10)
            bus = spawn(['dbus-daemon', '--session', '--nofork', '--print-address=1', '--nopidfile'], read=False)
            if not select.select([bus.stdout], [], [], 5)[0]:
                raise CheckFailure('dbus_startup')
            env['DBUS_SESSION_BUS_ADDRESS'] = bus.stdout.readline().decode().strip()
            threading.Thread(target=read_logs, args=(bus,), daemon=True).start()
            os.environ.update({name: env[name] for name in ('DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR', 'GSETTINGS_BACKEND')})
            import pyatspi
            from gi.repository import Gio, GLib
            proxy = Gio.DBusProxy.new_for_bus_sync(Gio.BusType.SESSION, Gio.DBusProxyFlags.NONE, None,
                'org.a11y.Bus', '/org/a11y/bus', 'org.freedesktop.DBus.Properties', None)
            for name in ('IsEnabled', 'ScreenReaderEnabled'):
                proxy.call_sync('Set', GLib.Variant('(ssv)', ('org.a11y.Status', name, GLib.Variant('b', True))), Gio.DBusCallFlags.NONE, 5000, None)
            server = spawn([str(signaling), f'127.0.0.1:{args.port}', str(operator/'server.pem'), str(operator/'server.key')])
            def server_ready():
                if server.poll() is not None:
                    raise CheckFailure('wss_operator_exited')
                try:
                    with socket.create_connection(('127.0.0.1', args.port), .2):
                        return True
                except OSError:
                    return False
            wait_for(server_ready, 'wss_operator_ready', 10)
            checks['real_wss_operator_with_trusted_synthetic_ca'] = True

            class Window:
                def __init__(self, name):
                    self.root = fixture/name; self.root.mkdir(mode=0o700)
                    self.env = env.copy()
                    for part, variable in [('config','XDG_CONFIG_HOME'),('data','XDG_DATA_HOME'),('cache','XDG_CACHE_HOME'),('runtime','XDG_RUNTIME_DIR'),('tmp','TMPDIR')]:
                        path=self.root/part;path.mkdir(mode=0o700);self.env[variable]=str(path)
                    self.process = spawn([str(binary)], self.env)
                    windows.append(self)
                    self.xid = wait_for(lambda: subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(self.process.pid),'--name','^Veyra$'],env=env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=5).stdout.splitlines(), 'native_window_'+name)[0].decode()
                    wait_for(lambda: self.button('Configurações'), 'accessible_document_'+name)
                def focus(self):
                    xdo('windowraise', self.xid)
                    xdo('windowfocus', '--sync', self.xid)
                    time.sleep(.05)
                def nodes(self):
                    desktop=pyatspi.Registry.getDesktop(0)
                    roots=[]
                    for child in desktop:
                        try:
                            if child.get_process_id()==self.process.pid: roots.append(child)
                        except Exception: pass
                    queue=[(node,0) for node in roots];count=0
                    while queue and count<2048:
                        node,depth=queue.pop(0);count+=1;yield node
                        if depth>=24:continue
                        try:queue.extend((node[index],depth+1) for index in range(min(node.childCount,512)))
                        except Exception:pass
                def text(self):
                    result=[]
                    for node in self.nodes():
                        try:
                            if node.name:result.append(node.name[:8192])
                            try:
                                value=node.queryText().getText(0,-1)
                                if value:result.append(value[:8192])
                            except Exception:pass
                        except Exception:pass
                    return '\n'.join(result)
                def find(self, predicate):
                    for node in self.nodes():
                        try:
                            if predicate(node):return node
                        except Exception:pass
                    return None
                def button(self, name):
                    def matches(node):
                        if node.getRole()!=pyatspi.ROLE_PUSH_BUTTON:
                            return False
                        accessible_name=' '.join(node.name.split())
                        if accessible_name==name:
                            return True
                        # This real navigation label includes the current unread
                        # badge once a message arrives, especially in release.
                        return name=='Mensagens pendentes' and re.fullmatch(re.escape(name)+r'\s*\d+',accessible_name) is not None
                    return self.find(matches)
                def press(self, name):
                    self.focus()
                    node=wait_for(lambda:self.button(name),'button_'+name,15)
                    try:
                        action=node.queryAction()
                        if not action.nActions or not action.doAction(0):raise CheckFailure('action_'+name)
                    except Exception:raise CheckFailure('action_'+name) from None
                    time.sleep(.08)
                def named_text(self, name):
                    return self.find(lambda node: node.name.strip()==name and node.queryText())
                def read_field(self,name):
                    node=wait_for(lambda:self.named_text(name),'field_'+name,15)
                    return node.queryText().getText(0,-1)
                def type_field(self, prefix, value):
                    self.focus()
                    node=wait_for(lambda:self.find(lambda n:n.getState().contains(pyatspi.STATE_EDITABLE)),'editable_'+prefix,15)
                    node.queryComponent().scrollTo(pyatspi.SCROLL_ANYWHERE)
                    node.queryComponent().grabFocus()
                    time.sleep(.15)
                    rect=node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
                    xdo('mousemove',str(rect.x+max(5,rect.width//2)),str(rect.y+max(5,rect.height//2)))
                    xdo('click','1')
                    time.sleep(.1)
                    xdo('key','--clearmodifiers','ctrl+a')
                    xdo('type','--clearmodifiers','--delay','1','--file','-',text=value)
                    wait_for(lambda:node.queryText().getText(0,-1)==value,'typed_'+prefix,10)
                def check_box(self, prefix=None):
                    self.focus()
                    node=wait_for(lambda:self.find(lambda n:n.getRole()==pyatspi.ROLE_CHECK_BOX and (prefix is None or n.name.startswith(prefix))),'checkbox',10)
                    action=node.queryAction()
                    if not action.doAction(0):raise CheckFailure('checkbox_action')
                def start(self):
                    self.press('Iniciar sessão')
                    wait_for(lambda:child_vaults(self.process.pid)[0],'real_vault_started')
                    wait_for(lambda:'Identidade ativa nesta execução' in self.text(),'active_identity')
                    self.press('Convites')
                    wait_for(lambda: self.button('Criar convite') and self.button('Criar convite').getState().contains(pyatspi.STATE_ENABLED),'wss_configured')
                def end(self):
                    self.press('Sair e descartar sessão')
                    wait_for(lambda:not child_vaults(self.process.pid)[0],'vault_terminated')
                    wait_for(lambda:'Sessão não iniciada' in self.text(),'session_ended')

            a,b=Window('a'),Window('b')
            a.start();b.start()
            checks['two_native_ui_sessions_and_separate_vaults_started'] = True
            checks['operator_configured_automatically_without_manual_ui_input'] = True
            fp_pattern=r'\b[0-9A-F]{4}(?: [0-9A-F]{4}){15}\b'
            for window in (a,b):
                identity=wait_for(lambda w=window:re.search(fp_pattern,w.text()),'native_identity').group(0)
                needles.append(identity.encode())
            a.press('Criar convite')
            token=wait_for(lambda:(value if (value:=a.read_field('Convite temporário para compartilhar')).startswith('veyra1:') else None),'native_invitation_created')
            needles.append(token.encode())
            b.type_field('Convite',token)
            b.press('Iniciar pareamento')
            a.press('Contatos da sessão')
            wait_for(lambda:a.button('Verificar identidade'),'a_noise_peer',25)
            wait_for(lambda:b.button('Verificar identidade'),'b_noise_peer',25)
            checks['real_invitation_and_noise_pairing_from_ui'] = True
            a.press('Verificar identidade');b.press('Verificar identidade')
            code_pattern=r'\b[0-9A-F]{4}(?:-[0-9A-F]{4}){3}\b'
            ca=wait_for(lambda:re.search(code_pattern,a.text()),'a_transcript_code').group(0)
            cb=wait_for(lambda:re.search(code_pattern,b.text()),'b_transcript_code').group(0)
            if ca!=cb:raise CheckFailure('transcript_codes_disagree')
            checks['native_transcript_codes_equal_before_approval'] = True
            for window in (a,b):
                window.check_box('Comparei o código')
                window.press('Aprovar identidade')
                wait_for(lambda w=window:'Identidade verificada' in w.text(),'native_identity_approved')
            checks['explicit_local_identity_approval_both_windows'] = True
            for window in (a,b):
                wait_for(lambda w=window:'P2P direto' in w.text(),'real_direct_ice_route')
            checks['both_ui_routes_report_real_direct_ice'] = True
            # The UI never queues plaintext while waiting for a key offer. Allow
            # the now-authenticated peers to exchange their one-time offers.
            time.sleep(1.2)

            def exchange(source,target,body,label):
                needles.append(body.encode())
                source.press('Mensagens pendentes')
                source.press('Nova mensagem')
                source.check_box()
                source.type_field('Mensagem',body)
                source.press('Cifrar e enviar')
                wait_for(lambda:source.read_field('Mensagem')=='','composer_cleared_'+label)
                checks['composer_cleared_after_native_send_'+label] = True
                wait_for(lambda:source.button('Fechar'),'composer_result_'+label)
                source.press('Fechar')
                target.press('Mensagens pendentes')
                wait_for(lambda:target.button('Abrir uma vez'),'pending_encrypted_'+label,20)
                if body in target.text():raise CheckFailure('plaintext_before_authorization_'+label)
                checks['no_plaintext_before_local_authorization_'+label] = True
                target.press('Abrir uma vez')
                wait_for(lambda:target.button('Consumir e abrir'),'local_confirmation_'+label)
                if body in target.text():raise CheckFailure('plaintext_before_confirmation_'+label)
                target.press('Consumir e abrir')
                wait_for(lambda:body in target.text(),'native_hpke_open_'+label,15)
                checks['native_hpke_body_displayed_once_'+label] = True
                source.focus()
                wait_for(lambda:body not in target.text(),'blur_hides_body_'+label)
                checks['native_focus_loss_hides_body_'+label] = True
                target.focus()
                wait_for(lambda:'Nenhuma mensagem pendente' in target.text() and not target.button('Abrir uma vez'),'no_reopen_card_'+label)
                checks['consumed_card_removed_without_history_'+label] = True
                if body in source.text() or body in target.text():raise CheckFailure('body_retained_in_accessible_ui_'+label)
            exchange(a,b,'VEYRA-SYNTHETIC-FORWARD-READ-ONCE-20261109','forward')
            exchange(b,a,'VEYRA-SYNTHETIC-REVERSE-READ-ONCE-20261109','reverse')
            for window in (a,b):window.end()
            checks['both_sessions_end_and_vaults_terminate'] = True
            if any(needle in logs for needle in needles):raise CheckFailure('fixture_or_invite_in_captured_process_output')
            checks['fixture_and_invite_absent_from_captured_process_output'] = True
        except CheckFailure as error:
            failure=str(error)
        except Exception as error:
            failure=type(error).__name__
        finally:
            if failure:
                for window in windows:
                    try:
                        state_text = window.text()
                        failure_state.append({'fixture': window.root.name,
                            'processExitCode': window.process.poll(),
                            'accessibleNodeCount': sum(1 for _ in window.nodes()),
                            'navigationAvailable': bool(window.button('Mensagens pendentes')),
                            'pendingOpenAvailable': bool(window.button('Abrir uma vez')),
                            'fatalInterfaceMessage': 'Falha na interface.' in state_text,
                            'openViewerMessage': 'Leitura única' in state_text,
                            'verificationDialogMessage': 'Compare este código' in state_text})
                    except Exception:
                        failure_state.append({'fixture': window.root.name, 'diagnosticUnavailable': True})
            for process in reversed(processes):
                if process.poll() is None:
                    try:os.killpg(process.pid,signal.SIGTERM)
                    except ProcessLookupError:pass
            for process in processes:
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid,signal.SIGKILL);process.wait()
        profiles={name:inspect_files(fixture/name,needles) for name in ('a','b') if (fixture/name).exists()}
        if any(profile['identityMetadataMatches'] for profile in profiles.values()):failure=failure or 'fixture_or_invite_in_inspected_profile'
        elif needles and profiles:checks['fixture_and_invite_absent_from_inspected_profiles']=True
        report={'checks':checks,'failure':failure,'failureState':failure_state,'profiles':profiles,'capturedOutputBytes':len(logs),
                'limitations':['Direct route on one isolated Linux container; no NAT, Windows or installer test.',
                    'No desktop TURN route test in this harness; separate core/process relay evidence is required.',
                    'Scans cover whole fixture/invite byte strings in isolated profiles and bounded stdout/stderr, not every residual copy or OS location.']}
        if args.report:
            args.report.parent.mkdir(parents=True,exist_ok=True)
            args.report.write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'passedChecks':len(checks),'checks':checks,'failure':failure,
                          'profileFiles':{name:len(profile['files']) for name,profile in profiles.items()},'failureState':failure_state},indent=2))
        if failure:raise SystemExit(1)


if __name__=='__main__':main()
