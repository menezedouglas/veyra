#!/usr/bin/env python3
"""Package the exact local datachannel source patch with deterministic metadata.

Run only after the vendor patch is finalized. No Cargo, network, application
process or runtime data is used. Original source and notice bytes are unchanged.
"""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "vendor/datachannel"
DESTINATION = ROOT / "infrastructure/licenses/datachannel-0.16.1-veyra-source.tar.gz"
PREFIX = "datachannel-0.16.1-veyra-source"
REQUIRED = {
    "Cargo.toml", "Cargo.toml.orig", "LICENSE", ".cargo_vcs_info.json",
    "README.md", "README.veyra.md", "src/lib.rs", "src/error.rs",
}
EXCLUDED = {"target", "cache", ".cache", "__pycache__", ".git", "node_modules"}


def snapshot():
    if not SOURCE.is_dir() or SOURCE.is_symlink():
        raise SystemExit("Finalized vendor/datachannel source directory is required.")
    files = {}
    for path in sorted(SOURCE.rglob("*")):
        relative = path.relative_to(SOURCE)
        if any(part in EXCLUDED for part in relative.parts):
            continue
        if path.is_symlink():
            raise SystemExit("Source symlinks are not packaged: " + relative.as_posix())
        if path.is_file():
            files[relative.as_posix()] = path.read_bytes()
    missing = REQUIRED - files.keys()
    if missing:
        raise SystemExit("Finalized source files are missing: " + ", ".join(sorted(missing)))
    return files


def archive_bytes(files):
    directories = {PREFIX}
    for name in files:
        parent = (Path(PREFIX) / name).parent
        while parent.as_posix() != ".":
            directories.add(parent.as_posix())
            parent = parent.parent
    output = io.BytesIO()
    # No source path, clock, original uid/gid, or gzip filename enters metadata.
    with gzip.GzipFile(filename="", fileobj=output, mode="wb", compresslevel=9, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
            for name in sorted(directories | {PREFIX + "/" + name for name in files}):
                info = tarfile.TarInfo(name)
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ""
                if name in directories:
                    info.type = tarfile.DIRTYPE
                    info.mode = 0o755
                    archive.addfile(info)
                else:
                    content = files[name[len(PREFIX) + 1:]]
                    info.mode = 0o644
                    info.size = len(content)
                    archive.addfile(info, io.BytesIO(content))
    return output.getvalue()


def main():
    files = snapshot()
    content = archive_bytes(files)
    if snapshot() != files:
        raise SystemExit("Vendor source changed during packaging; finalize the patch and retry.")
    DESTINATION.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=DESTINATION.parent, prefix=".datachannel-source-", delete=False) as output:
            temporary = Path(output.name)
            output.write(content)
        temporary.chmod(0o644)
        os.replace(temporary, DESTINATION)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()
    print(json.dumps({"archive": str(DESTINATION.relative_to(ROOT)), "files": len(files),
                      "sha256": hashlib.sha256(content).hexdigest()}, sort_keys=True))


if __name__ == "__main__":
    main()
