#!/usr/bin/env python3
"""Check the bundled local command authority and production CSP."""
import json
from pathlib import Path

root=Path(__file__).resolve().parents[2]
base=root/'apps/desktop/src-tauri'
cap=json.loads((base/'capabilities/main.json').read_text(encoding='utf-8'))
conf=json.loads((base/'tauri.conf.json').read_text(encoding='utf-8'))
allowed={f'allow-{name}' for name in ('start-session','create-invite','join-invite','verify-peer','send-message','list-pending','open-once','discard-message','revoke-peer','connection-status','end-session')}
assert set(cap['permissions'])==allowed
assert cap['windows']==['main'] and cap['local'] is True and 'remote' not in cap
csp=conf['app']['security']['csp']
for value in ("default-src 'none'", "script-src 'self'", "object-src 'none'", "frame-src 'none'", "form-action 'none'"):
    assert value in csp
assert 'https:' not in csp and 'unsafe-eval' not in csp
assert conf['build']['devUrl'] == 'http://127.0.0.1:1420'
dev_csp = conf['app']['security']['devCsp']
dev_connect = next(part.strip().split()[1:] for part in dev_csp.split(';') if part.strip().startswith('connect-src '))
assert set(dev_connect) == {'ipc:', 'http://ipc.localhost', 'http://127.0.0.1:1420', 'ws://127.0.0.1:1420'}
for window in conf['app']['windows']:
    assert window['title']=='Veyra' and window['incognito'] and not window['devtools']
print('PASS: local command allowlist and production CSP configuration')
