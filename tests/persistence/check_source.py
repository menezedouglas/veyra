#!/usr/bin/env python3
"""Regression guard for application-controlled persistence/logging surfaces.

The native public-contact catalog is the only application-data write exception.
This source check does not audit WebView/OS writes. See docs/testing.md.
"""
from pathlib import Path
import re

root=Path(__file__).resolve().parents[2]
rules={
    "browser storage": r"\b(?:localStorage|sessionStorage|indexedDB|caches)\s*[.(]",
    "service worker": r"navigator\s*\.\s*serviceWorker",
    "unsafe message HTML": r"\bv-html\s*=|\.innerHTML\s*=",
    "console logging": r"\bconsole\s*\.\s*(?:log|debug|info|warn|error)\s*\(",
    "filesystem persistence": r"\b(?:(?:std|tokio)::)?fs::(?:write|File|OpenOptions|create_dir(?:_all)?|rename|copy|hard_link|remove_file)\b",
    "sensitive runtime logging": r"\b(?:println|eprintln|dbg)!\s*\(",
}
errors=[]
paths=(list((root/'apps/desktop/src').rglob('*'))
       +list((root/'apps/desktop/src-tauri/src').rglob('*.rs'))
       +list((root/'crates').rglob('*.rs')))
for path in paths:
    if not path.is_file() or '.test.' in path.name or 'tests' in path.relative_to(root).parts:continue
    if path.name == 'build.rs':continue  # Cargo directives are build output, not runtime logging.
    # This laboratory-only binary uses stdout as inherited controller IPC;
    # it is not bundled in Veyra and carries synthetic fixtures only.
    if path.relative_to(root).as_posix() == 'crates/transport-webrtc/src/bin/veyra-transport-probe.rs':continue
    if path.suffix not in ('.ts','.vue','.rs'):continue
    source=path.read_text(encoding='utf-8')
    # Test modules may print synthetic fixtures through the test harness only.
    source=source.split('#[cfg(test)]')[0]
    for label,pattern in rules.items():
        # ADR 0004 permits only public-contact metadata in this native module.
        # Do not exempt it from browser storage, HTML or logging restrictions.
        # Bootstrap main.rs only resolves app_data_dir and opens ContactBook;
        # it has no filesystem-write exception of its own.
        if (label == 'filesystem persistence'
            and path.relative_to(root).as_posix() == 'apps/desktop/src-tauri/src/contacts.rs'):
            continue
        if re.search(pattern,source):errors.append(f'{path.relative_to(root)}: {label}')
if errors:
    raise SystemExit('\n'.join(errors))
print('PASS: message persistence/logging guards; native public-contact catalog allowed (not a WebView/OS audit)')
