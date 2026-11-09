# Veyra local correction to datachannel 0.16.1

This directory preserves the published `datachannel` 0.16.1 crate, including
its original `LICENSE` (MPL-2.0), `.cargo_vcs_info.json`, `Cargo.toml.orig`,
normalized `Cargo.toml`, source, tests and README. The local modifications
remain covered by MPL-2.0. This is a Veyra correction; no upstream fix,
acceptance, advisory or release is claimed.

Origin: <https://crates.io/crates/datachannel/0.16.1>.
The SHA-256 of the original `.crate` archive is
`c29e35372e6c3ba1c5a30d3c0a06af13a251149a0bc044abd10acd30dd733094`,
matching the pre-patch registry entry in the workspace lockfile.
The original VCS metadata records repository
<https://github.com/lerouxrgd/datachannel-rs> and commit
`6abf0f8a461f6eba8bef51c011141eada40bb297`, with an empty `path_in_vcs`.

## Failure and correction

`RtcPeerConnection::selected_candidate_pair()` allocates both C output buffers
at the maximum size returned by `rtcGetSelectedCandidatePair`. That API writes
each candidate followed by its own terminating NUL. If their string lengths
differ, the shorter candidate has unused zero-filled bytes after that NUL.
The original `ffi_string` calls `CStr::from_bytes_with_nul` on the whole
buffer. That rejects the interior NUL, so a real selected pair is reported as
`None`. Repeating the getter does not fix the unequal lengths. Veyra therefore
could display `Unknown` while the established DataChannel carried messages.

The helper now uses bounded `CStr::from_bytes_until_nul`, preserving UTF-8
validation and rejecting a buffer without a terminator. `src/error.rs` maps
the new standard-library error into the existing `Error::BadString` variant.
No pointer scan beyond the provided slice, new unsafe code, candidate
inference or transport behavior is introduced.

The regression in `src/lib.rs` reproduces the C API's maximum-sized buffers
with synthetic, unequal-length host and relay candidates, in both orders.
It fails with the original helper. Two other cases preserve rejection of
unterminated buffers and invalid UTF-8 before the terminator.
`python3 tests/security/datachannel_padding.py --prove-original-failure` compiles
the actual helper, error module and co-located tests with `rustc --test`, without
copying the implementation or resolving the excluded crate's unrelated dev graph.
The unequal-length case failed with the original helper; all three cases passed
with the correction. This tests the conversion contract, not the entire crate.

Only `src/lib.rs` and `src/error.rs` differ from published crate files;
this README is additional. The workspace applies the patch with
`[patch.crates-io]` and excludes this package from workspace membership.

The libdatachannel C API implementation used by this build is
[v0.23.2 capi.cpp](https://github.com/paullouisageneau/libdatachannel/blob/9e6a13abbb6846c003d817d0387b6706466e2b03/src/capi.cpp).
The safe conversion contract is documented by
[Rust CStr](https://doc.rust-lang.org/std/ffi/struct.CStr.html#method.from_bytes_until_nul).
Remove this local patch after a compatible upstream release is reviewed and
the unequal-length regression passes against it.
