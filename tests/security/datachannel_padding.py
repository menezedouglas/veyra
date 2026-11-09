#!/usr/bin/env python3
"""Run the actual vendored FFI helper and its tests with the standard library.

Cargo cannot test an excluded dependency with its unpublished dev graph from
this workspace. This extracts the unchanged helper/test source, rather than
maintaining a second implementation or resolving unrelated test dependencies.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--prove-original-failure', action='store_true')
args = parser.parse_args()
source = (ROOT / 'vendor/datachannel/src/lib.rs').read_text()
start = source.index('fn ffi_string(')
end = source.index('/// An optional function to enable libdatachannel logging via `tracing`', start)
actual = source[start:end]
assert 'mod ffi_string_tests' in actual
assert actual.count('CStr::from_bytes_until_nul(ffi)') == 1
error_source = json.dumps(str(ROOT / 'vendor/datachannel/src/error.rs'), ensure_ascii=False)
prefix = '#![allow(dead_code)]\nmod error { include!(' + error_source + '); }\n'

with tempfile.TemporaryDirectory(prefix='veyra-datachannel-regression-') as directory:
    folder = Path(directory)

    def run(code, name):
        source_path = folder / (name + '.rs')
        executable = folder / name
        source_path.write_text(prefix + code)
        subprocess.run(['rustc', '--edition=2021', '--test', str(source_path), '-o', str(executable)], check=True)
        return subprocess.run([str(executable)], capture_output=True, text=True)

    if args.prove_original_failure:
        original = actual.replace('CStr::from_bytes_until_nul(ffi)', 'CStr::from_bytes_with_nul(ffi)', 1)
        before = run(original, 'original')
        assert before.returncode != 0, before.stdout + before.stderr
        assert 'ffi_string_tests::selected_candidate_pair_accepts_unequal_length_buffers ... FAILED' in before.stdout, before.stdout
        print('PASS: original helper fails the unequal-length selected-pair regression')

    after = run(actual, 'corrected')
    print(after.stdout, end='')
    assert after.returncode == 0, after.stderr
