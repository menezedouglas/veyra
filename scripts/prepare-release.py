#!/usr/bin/env python3
"""Assemble unsigned release assets and checksums from validated CI artifacts."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys


ROOT = Path(__file__).resolve().parents[1]
VERSION_PATTERN = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("Duplicate JSON key in release metadata.")
        value[key] = item
    return value


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)


def validate_evidence(path):
    package = read_json(ROOT / "package.json")
    version = package.get("version") if isinstance(package, dict) else None
    if not isinstance(version, str) or not re.fullmatch(VERSION_PATTERN, version):
        raise ValueError("The package manifest must contain a valid X.Y.Z version.")
    report = read_json(path)
    if not isinstance(report, dict) or type(report.get("schema_version")) is not int or report["schema_version"] != 1:
        raise ValueError("Release evidence must use schema_version 1.")
    if report.get("passed") is not True:
        raise ValueError("Release evidence did not pass the release gate.")
    if report.get("channel") not in ("experimental", "production"):
        raise ValueError("Release evidence channel must be experimental or production.")
    if report.get("version") != version or report.get("tag") != "v" + version:
        raise ValueError("Release evidence version and tag must match package.json.")
    if type(report.get("ready_for_production")) is not bool:
        raise ValueError("Release evidence must contain boolean ready_for_production.")
    pending = report.get("pending")
    if (not isinstance(pending, list)
            or any(not isinstance(name, str) or not re.fullmatch(r"[a-z][a-z0-9_]*", name) for name in pending)
            or len(set(pending)) != len(pending)):
        raise ValueError("Release evidence pending items must be unique acceptance names.")
    if report.get("errors") != []:
        raise ValueError("Release evidence contains validation errors.")
    if report["ready_for_production"] != (not pending):
        raise ValueError("Release evidence production status contradicts its pending acceptance items.")
    if report["channel"] == "production" and not report["ready_for_production"]:
        raise ValueError("Production release evidence is not ready for production.")
    return report


def installer_assets(directory, version):
    if not directory.is_dir():
        raise ValueError("The downloaded artifacts directory does not exist.")
    assets = sorted(path for path in directory.rglob("*")
                    if path.is_file() and (path.suffix == ".deb" or path.name.endswith("-setup.exe")))
    names = set()
    counts = {"deb": 0, "nsis": 0}
    for path in assets:
        if path.name.casefold() in names:
            raise ValueError("Duplicate installer asset basename: " + path.name)
        names.add(path.name.casefold())
        kind = "deb" if path.suffix == ".deb" else "nsis"
        ending = r"\.deb" if kind == "deb" else r"-setup\.exe"
        if not re.fullmatch(r"Veyra_" + re.escape(version) + r"_[A-Za-z0-9]+" + ending, path.name):
            raise ValueError("Installer filename must identify Veyra and the release version: " + path.name)
        counts[kind] += 1
    if counts != {"deb": 1, "nsis": 1}:
        raise ValueError("Expected exactly one Linux .deb and one Windows *-setup.exe installer.")
    return assets


def release_notes(report):
    lines = [
        f"# Veyra {report['tag']}",
        "",
        f"Channel: **{report['channel']}**. Packages are **unsigned**.",
        "",
    ]
    if report["channel"] == "experimental":
        lines += ["**Experimental packages are not approved for production.**", ""]
    lines += [
        f"Production acceptance: **{'complete' if report['ready_for_production'] else 'incomplete'}**.",
        "",
        "Review `SHA256SUMS` and the attached `release-evidence.json` before publishing this draft.",
        "Checksums verify downloaded bytes; they do not provide a publisher signature.",
        "Builds and automated tests do not replace installer validation, desktop UI and privacy testing, "
        "cross-platform interoperability checks or an independent security review.",
        "No independent audit is claimed.",
        "",
    ]
    if report["pending"]:
        lines += ["Pending acceptance items:", ""]
        lines += [f"- `{name}`" for name in report["pending"]]
    else:
        lines += ["All mandatory acceptance items are recorded complete; independent review still applies."]
    return "\n".join(lines) + "\n"


def prepare(artifacts, evidence, output, notes):
    report = validate_evidence(evidence)
    assets = installer_assets(artifacts, report["version"])
    if output.exists() or output.is_symlink():
        raise ValueError("The release output directory already exists; refusing to mix stale assets.")
    if notes.exists() or notes.is_symlink():
        raise ValueError("The release notes file already exists; refusing to overwrite it.")
    output.mkdir()
    for source in assets:
        shutil.copy2(source, output / source.name)
    shutil.copy2(evidence, output / "release-evidence.json")
    checksums = []
    for asset in sorted(output.iterdir()):
        with asset.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        checksums.append(digest + "  " + asset.name)
    (output / "SHA256SUMS").write_text("\n".join(checksums) + "\n", encoding="utf-8", newline="\n")
    with notes.open("x", encoding="utf-8", newline="\n") as stream:
        stream.write(release_notes(report))
    print(f"Prepared {len(assets)} unsigned installers and release evidence for {report['tag']} ({report['channel']}).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--notes", required=True, type=Path)
    args = parser.parse_args()
    try:
        prepare(args.artifacts, args.evidence, args.output, args.notes)
    except (OSError, ValueError) as error:
        print(f"Cannot prepare release: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
