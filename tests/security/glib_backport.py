#!/usr/bin/env python3
from pathlib import Path
root=Path(__file__).resolve().parents[2]
source=(root/'vendor/glib/src/variant_iter.rs').read_text()
body=source.split('fn impl_get(&self, i: usize)')[1].split('impl Iterator')[0]
assert 'let mut p: *mut libc::c_char' in body
assert '&mut p,' in body
assert '\n                &p,' not in body
assert 'glib = { path = "vendor/glib" }' in (root/'Cargo.toml').read_text()
print('PASS: upstream GLib mutable out-pointer backport selected')
