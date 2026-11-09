#!/usr/bin/env python3
"""Collect license texts for the installed frontend production dependency graph.
Run after pnpm install when production dependency inputs change. The result is
packaged as a static asset, without any runtime network access.
"""
from pathlib import Path
import json
import re

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / 'apps/desktop'

def resolve(origin, name):
    for parent in [origin, *origin.parents]:
        candidate = parent / 'node_modules' / name
        if (candidate / 'package.json').is_file():
            return candidate.resolve()
    raise RuntimeError(f'Unresolved production dependency: {name}')

seen = {}
def visit(origin, name):
    package = resolve(origin, name)
    manifest = json.loads((package / 'package.json').read_text())
    key = (manifest['name'], manifest['version'])
    if key in seen: return
    seen[key] = (package, manifest)
    for dependency in sorted(manifest.get('dependencies', {})):
        visit(package, dependency)

for name in sorted(json.loads((APP/'package.json').read_text())['dependencies']):
    visit(APP, name)
sections = ['Veyra — frontend dependency notices\n\nGenerated from installed production dependencies. The graph can include code\nremoved by the production bundler. This is attribution, not a legal assessment.\n']
missing=[]
for (name, version), (package, manifest) in sorted(seen.items()):
    files = sorted(p for p in package.iterdir() if p.is_file() and re.match(r'^(licen[cs]e|notice)(\.|$)', p.name, re.I))
    sections.append(f'\n{"="*72}\n{name} {version}\nLicense declaration: {manifest.get("license", "UNDECLARED")}\n')
    if not files:
        missing.append(name)
        sections.append('No root license text found. Review source package before release.\n')
    for path in files:
        sections.append(f'\n--- {path.name} ---\n{path.read_text(errors="replace")}\n')
output=APP/'public/licenses/THIRD-PARTY-NOTICES.txt'
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(''.join(sections))
print(f'Collected notices for {len(seen)} frontend production packages.')
if missing:
    raise SystemExit('License text requires review: '+', '.join(missing))
