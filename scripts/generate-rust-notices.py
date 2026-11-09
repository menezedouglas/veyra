#!/usr/bin/env python3
"""Preserve packaged third-party notices; this is not a license audit.

Run where Cargo and its resolved source cache are available. The only Cargo
operation is metadata --locked (no builds/tests). Original notice bytes are
copied without rewriting their copyright holders or license terms.
"""
import argparse
import json
import re
import subprocess
import hashlib
from collections import deque
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ROOT_PACKAGES = ("veyra-desktop", "veyra-daemon", "veyra-signaling")
NOTICE_NAME = re.compile(r"^(licen[cs]e|copyright|copying|notice)", re.I)


def notice_files(directory, explicit=None):
    result = set()
    directory = directory.resolve()
    for path in directory.rglob("*"):
        if not path.is_file() or not path.resolve().is_relative_to(directory):
            continue
        relative = path.relative_to(directory)
        if any(part in {"target", ".git", "node_modules"} for part in relative.parts):
            continue
        if NOTICE_NAME.match(path.name) or any(part.lower() in {"license", "licence", "licenses", "licences"} for part in relative.parts[:-1]):
            result.add(path)
    if explicit:
        path = Path(explicit)
        if not path.is_absolute():
            path = directory / path
        if path.is_file() and path.resolve().is_relative_to(directory):
            result.add(path)
    return sorted(result, key=lambda path: str(path.relative_to(directory)))


def section(output, title, fields, files, directory):
    output.write(("\n" + "=" * 78 + "\n" + title + "\n" + "=" * 78 + "\n").encode())
    for key, value in fields.items():
        output.write((str(key) + ": " + str(value) + "\n").encode())
    if not files:
        output.write(b"No packaged license/notice file found; see the generated report.\n")
    for path in files:
        relative = path.relative_to(directory)
        output.write(("\n--- BEGIN ORIGINAL " + str(relative) + " ---\n").encode())
        original = path.read_bytes()
        output.write(original)
        if not original.endswith(b"\n"):
            output.write(b"\n")
        output.write(("--- END ORIGINAL " + str(relative) + " ---\n").encode())


def selected_packages(metadata, roots):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root_ids = {package["id"] for package in packages.values() if package["name"] in roots}
    absent = set(roots) - {packages[pid]["name"] for pid in root_ids}
    if absent:
        raise SystemExit("Missing workspace roots: " + ", ".join(sorted(absent)))
    queue = deque(root_ids)
    visited = set()
    while queue:
        pid = queue.popleft()
        if pid in visited:
            continue
        visited.add(pid)
        for dependency in nodes[pid]["deps"]:
            kinds = dependency.get("dep_kinds", [])
            if not kinds or any(kind.get("kind") in (None, "normal", "build") for kind in kinds):
                queue.append(dependency["pkg"])
    workspace = set(metadata["workspace_members"])
    return sorted((packages[pid] for pid in visited if pid not in workspace), key=lambda package: (package["name"], package["version"], package["id"]))


def vcs(package):
    path = Path(package["manifest_path"]).parent / ".cargo_vcs_info.json"
    return json.loads(path.read_text()) if path.is_file() else {}


def repository_root_notices(package, packages):
    """Use only an original repository-root notice at the SAME recorded commit.

    Preserve provenance and report that the published subcrate omitted a notice;
    do not infer legal compatibility or silently synthesize a license template.
    """
    revision = vcs(package).get("git", {}).get("sha1")
    if not revision:
        return None
    for candidate in packages:
        source = vcs(candidate)
        if source.get("git", {}).get("sha1") != revision or source.get("path_in_vcs") != "":
            continue
        directory = Path(candidate["manifest_path"]).parent
        files = sorted(p for p in directory.iterdir() if p.is_file() and NOTICE_NAME.match(p.name))
        if files:
            return directory, files, {"name": candidate["name"], "version": candidate["version"], "git_commit": revision, "repository": candidate.get("repository")}
    return None


def preserved_upstream_notices(package):
    directory = ROOT / "infrastructure/licenses/rust-upstream" / (package["name"] + "-" + package["version"])
    source = directory / "SOURCE.json"
    if not source.is_file():
        return None
    provenance = json.loads(source.read_text())
    revision = vcs(package).get("git", {}).get("sha1")
    if provenance.get("name") != package["name"] or provenance.get("version") != package["version"] or provenance.get("git_commit") != revision:
        raise SystemExit("Supplemental license provenance mismatch: " + package["name"])
    files = []
    for original in provenance["files"]:
        path = directory / original["filename"]
        if not path.resolve().is_relative_to(directory.resolve()) or hashlib.sha256(path.read_bytes()).hexdigest() != original["sha256"]:
            raise SystemExit("Original supplemental license changed: " + package["name"])
        files.append(path)
    return directory, files, provenance


def native_notices(metadata, destination):
    candidates = [package for package in metadata["packages"] if package["name"] == "datachannel-sys"]
    missing = []
    count = 0
    with destination.open("wb") as output:
        output.write(b"VEYRA THIRD-PARTY NATIVE SOURCE NOTICES\n\n")
        output.write(b"Original notices shipped in datachannel-sys' packaged libdatachannel sources.\n")
        output.write(b"This deliberately includes optional source components (libjuice, media/examples)\n")
        output.write(b"even where USE_NICE=ON, NO_MEDIA=ON and NO_EXAMPLES=ON do not compile them.\n")
        output.write(b"It is notice preservation, not a legal compatibility audit or complete native SBOM.\n")
        if not candidates:
            missing.append({"component": "datachannel-sys", "reason": "not in Cargo metadata"})
        for package in candidates:
            source = Path(package["manifest_path"]).parent
            components = {
                "datachannel-sys": source,
                "libdatachannel": source / "libdatachannel",
                "usrsctp": source / "libdatachannel/deps/usrsctp",
                "libsrtp": source / "libdatachannel/deps/libsrtp",
                "plog": source / "libdatachannel/deps/plog",
                "libjuice": source / "libdatachannel/deps/libjuice",
                "nlohmann-json": source / "libdatachannel/deps/json",
            }
            for name, directory in components.items():
                # Component-root notices suffice here; nested components get
                # their own sections, preventing silent ownership merging.
                files = sorted([p for p in directory.iterdir() if p.is_file() and NOTICE_NAME.match(p.name)]) if directory.exists() else []
                provenance = {"Packaged source": package["name"] + " " + package["version"], "Cargo source": package.get("source") or "local"}
                if name == "nlohmann-json" and not files and package["version"] == "0.23.0+0.23.2":
                    directory = ROOT / "infrastructure/licenses/nlohmann-json-v3.12.0"
                    files = sorted(directory.glob("LICENSE*")) if directory.exists() else []
                    provenance.update({"Version": "3.12.0", "Git commit": "55f93686c01528224f448c19128836e7df245f72", "Original source": "https://raw.githubusercontent.com/nlohmann/json/55f93686c01528224f448c19128836e7df245f72/LICENSE.MIT", "Build scope": "dependency of examples, excluded by NO_EXAMPLES=ON; original notice deliberately preserved"})
                section(output, name, provenance, files, directory)
                count += len(files)
                if not files:
                    missing.append({"component": name, "packaged_source": package["name"] + " " + package["version"], "reason": "no original license file in packaged source; JSON examples are not built" if name == "nlohmann-json" else "no original license file in packaged source"})
        # Preserve public distribution copyright files of the primary dynamic
        # native libraries in the observed Linux build environment, when present.
        common = set()
        system_packages = {"libnice10", "libglib2.0-0", "libssl3", "libstdc++6", "libgcc-s1", "libc6"}
        libraries = list((ROOT / "target/release/build").glob("datachannel-sys-*/out/lib/libdatachannel.so")) or list((ROOT / "target/debug/build").glob("datachannel-sys-*/out/lib/libdatachannel.so"))
        if libraries:
            dependencies = subprocess.check_output(["ldd", str(libraries[0])], text=True)
            for filename in re.findall(r"(?:=>\s+|^\s*)(/\S+)", dependencies, re.M):
                path = Path(filename)
                variants = {str(path), str(path.resolve())}
                if filename.startswith("/lib/"):
                    variants.add("/usr" + filename)
                for variant in variants:
                    query = subprocess.run(["dpkg-query", "-S", variant], capture_output=True, text=True)
                    if query.returncode == 0:
                        system_packages.update(line.split(": ", 1)[0].split(":", 1)[0] for line in query.stdout.splitlines() if ": " in line)
                        break
        for package in sorted(system_packages):
            path = Path("/usr/share/doc") / package / "copyright"
            if not path.is_file():
                missing.append({"component": package, "reason": "system package copyright not found in this environment"})
                continue
            section(output, "Linux distribution package " + package, {"Scope": "observed build environment; not Windows licensing evidence"}, [path], path.parent)
            count += 1
            text = path.read_text(errors="replace")
            common.update(re.findall(r"/usr/share/common-licenses/([A-Za-z0-9_.+-]+)", text))
        for name in sorted(common):
            path = Path("/usr/share/common-licenses") / name.rstrip(".")
            if path.is_file():
                section(output, "Referenced distribution license " + path.name, {}, [path], path.parent)
                count += 1
            else:
                missing.append({"component": "common-license:" + name, "reason": "referenced text absent"})
    return count, missing


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", default="x86_64-unknown-linux-gnu")
    parser.add_argument("--metadata", type=Path, help="Use an already generated Cargo metadata JSON")
    parser.add_argument("--output-directory", type=Path, default=ROOT / "docs")
    args = parser.parse_args()
    invocation = ["cargo", "metadata", "--format-version", "1", "--locked", "--all-features", "--filter-platform", args.platform]
    metadata = json.loads(args.metadata.read_text() if args.metadata else subprocess.check_output(invocation, cwd=ROOT))
    selected = selected_packages(metadata, ROOT_PACKAGES)
    args.output_directory.mkdir(parents=True, exist_ok=True)
    missing = []
    packaged_missing = []
    supplemented = []
    files_count = 0
    rust = args.output_directory / "THIRD-PARTY-RUST.txt"
    with rust.open("wb") as output:
        output.write(b"VEYRA THIRD-PARTY RUST NOTICES\n\n")
        output.write(("Metadata command: " + " ".join(invocation) + "\nRoots: " + ", ".join(ROOT_PACKAGES) + "\n").encode())
        output.write(b"Traversal: normal/build dependency edges; dev edges excluded. All features and\n")
        output.write(b"Cargo workspace feature unification may overinclude optional/platform packages.\n")
        output.write(b"SPDX fields are copied metadata, not an assertion of legal compatibility.\n")
        output.write(b"Original notice bytes follow unmodified. No missing license text is invented.\n")
        for package in selected:
            directory = Path(package["manifest_path"]).parent
            files = notice_files(directory, package.get("license_file"))
            provenance = {"SPDX metadata": package.get("license") or "not declared", "Source": package.get("source") or "local vendored patch", "Repository": package.get("repository") or "not declared"}
            if not files:
                packaged_missing.append({"name": package["name"], "version": package["version"]})
                root_notice = repository_root_notices(package, metadata["packages"])
                if root_notice:
                    directory, files, origin = root_notice
                    provenance["Original notice provenance"] = json.dumps(origin, sort_keys=True)
                    provenance["Scope"] = "Published subcrate omitted a notice; original repository-root notices from the same commit preserved, without legal scope/compatibility conclusion"
                    supplemented.append({"name": package["name"], "version": package["version"], "original_notice_from": origin})
                else:
                    upstream = preserved_upstream_notices(package)
                    if upstream:
                        directory, files, origin = upstream
                        provenance["Original notice provenance"] = json.dumps(origin, sort_keys=True)
                        provenance["Scope"] = "Published crate omitted a notice. Original upstream/referenced texts preserved with exact revision and hashes; no legal scope or compatibility conclusion"
                        supplemented.append({"name": package["name"], "version": package["version"], "original_notice_from": origin})
            section(output, package["name"] + " " + package["version"], provenance, files, directory)
            files_count += len(files)
            if not files:
                missing.append({"name": package["name"], "version": package["version"], "spdx_metadata": package.get("license"), "repository": package.get("repository"), "reason": "no original license/notice file in packaged source"})
    native_count, native_missing = native_notices(metadata, args.output_directory / "THIRD-PARTY-NATIVE.txt")
    report = {"purpose": "Preserve original notices; legal compatibility and release review remain pending", "platform": args.platform, "roots": list(ROOT_PACKAGES), "cargo_command": invocation, "rust_packages": len(selected), "rust_notice_files": files_count, "native_notice_files": native_count, "rust_missing_notice_files": missing, "rust_packaged_missing_notice_files": packaged_missing, "rust_original_notices_supplemented": supplemented, "native_missing_notice_files": native_missing}
    (args.output_directory / "THIRD-PARTY-LICENSE-REPORT.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"rust_packages": len(selected), "rust_notice_files": files_count, "native_notice_files": native_count, "rust_missing": len(missing), "native_missing": len(native_missing)}, sort_keys=True))


if __name__ == "__main__":
    main()
