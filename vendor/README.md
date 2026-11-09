# Vendored dependencies

## datachannel FFI buffer correction

`datachannel/` preserves the published datachannel 0.16.1 source and license,
with a local correction for NUL-padded C string buffers and regression tests.
The exact archive checksum, VCS origin and changes are documented in
[README.veyra.md](datachannel/README.veyra.md). This correction has not been
attributed to an upstream patch or release.

## GLib compatibility security backport

`glib/` is the published glib 0.18.5 source, retaining its original copyright
and license files. The only source modification is the two-line upstream fix
from gtk-rs/gtk-rs-core commit
`b5a4071e439bef2b5eea76c3aa25e5ae84839e34`:
<https://github.com/gtk-rs/gtk-rs-core/pull/1343>.

`VariantStrIter::impl_get` now passes a mutable out-pointer to
`g_variant_get_child`. This backports the RUSTSEC-2024-0429 fix while retaining
the glib 0.18 API used by GTK 3/Tauri. The version remains 0.18.5; no nonexistent
upstream release is claimed. The package-version-based audit still reports
the advisory; it must be reviewed with this source diff and test evidence.
No audit ignore is configured.

Remove the patch when Tauri's GTK dependency chain provides a compatible fixed
release. This backport is not an audit of the whole dependency or application.
