#!/usr/bin/env python3
"""Build and stage native internal binaries; contains no user runtime data."""
import argparse,re,shutil,subprocess,os,sys
from pathlib import Path

root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser()
p.add_argument('--target')
p.add_argument('--debug',action='store_true')
p.add_argument('--native-libnice',action='store_true')
a=p.parse_args()
subprocess.run([sys.executable, str(root/'scripts/package-datachannel-source.py')], cwd=root, check=True)
target=a.target
if target is None:
    details=subprocess.check_output(['rustc','-vV'],text=True)
    target=next(line.split(': ',1)[1] for line in details.splitlines() if line.startswith('host: '))
if not re.fullmatch(r'[A-Za-z0-9_-]+',target):raise SystemExit('Invalid target')
profile='debug' if a.debug else 'release'
cmd=['cargo','build','--locked','-p','veyra-daemon']
if a.target:cmd+=['--target',target]
if a.native_libnice:cmd+=['--features','native-libnice']
if not a.debug:cmd.append('--release')
subprocess.run(cmd,cwd=root,check=True)
destination=root/'apps/desktop/src-tauri/binaries'
destination.mkdir(exist_ok=True)
suffix='.exe' if 'windows' in target else ''
for name in ['veyra-vault','veyra-daemon']:
    base=root/'target'/target if a.target else root/'target'
    source=base/profile/(name+suffix)
    shutil.copy2(source,destination/(name+'-'+target+suffix))
if a.native_libnice and 'linux' in target:
    libraries=list((base/profile/'build').glob('datachannel-sys-*/out/lib/libdatachannel.so.0.23'))
    if not libraries:raise SystemExit('Native libdatachannel library missing')
    native=destination/'native';native.mkdir(exist_ok=True)
    shutil.copy2(max(libraries,key=lambda p:p.stat().st_mtime),native/'libdatachannel.so.0.23')
    cargo_root=Path(os.environ.get('CARGO_HOME',str(Path.home()/'.cargo')))
    licenses=list(cargo_root.glob('registry/src/*/datachannel-sys-0.23.0+0.23.2/libdatachannel/LICENSE'))
    if len(licenses)!=1:raise SystemExit('Pinned native dependency license unavailable')
    shutil.copy2(licenses[0],native/'LICENSE-libdatachannel.txt')
print('Staged Veyra internal binaries for '+target)
