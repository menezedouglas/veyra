#!/usr/bin/env python3
"""Validate release metadata and preserve the separate production evidence gate."""

import argparse
import json
from pathlib import Path
import re
import sys
import tomllib


REQUIRED_EVIDENCE = (
    "desktop_end_to_end_direct",
    "desktop_end_to_end_turn_udp",
    "turn_tcp_tls_verified",
    "daemon_vault_process_boundary",
    "windows_named_pipe_acl",
    "linux_ipc_credentials",
    "desktop_privacy_audit_linux",
    "desktop_privacy_audit_windows",
    "windows_linux_interoperability",
    "windows_installer_verified",
    "linux_package_installed",
    "network_change_suspend_crash_tested",
    "operator_credentials_and_endpoints",
    "brand_registry_review",
    "signaling_operator_failover",
    "ice_restart_reconnection",
    "periodic_turn_credential_refresh",
    "windows_native_backend_packaged",
)
VERSION_PATTERN = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
VERSION_SOURCES = (
    ("package.json", ("version",)),
    ("apps/desktop/package.json", ("version",)),
    ("apps/desktop/src-tauri/tauri.conf.json", ("version",)),
    ("Cargo.toml", ("workspace", "package", "version")),
    ("apps/desktop/src-tauri/Cargo.toml", ("package", "version")),
)


def unique_object(pairs):
    """Reject duplicate JSON keys instead of silently overriding evidence."""
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON key")
        value[key] = item
    return value


def read_document(root, relative_path):
    source = (root / relative_path).read_text(encoding="utf-8")
    if relative_path.endswith(".toml"):
        return tomllib.loads(source)
    return json.loads(source, object_pairs_hook=unique_object)


def evaluate(root, channel, tag):
    errors = []
    completed = []
    pending = []
    try:
        matrix = read_document(root, "docs/release-readiness.json")
    except (OSError, ValueError):
        errors.append("Cannot read docs/release-readiness.json: invalid or unreadable JSON.")
    else:
        if not isinstance(matrix, dict):
            errors.append("Release readiness must be a JSON object.")
        elif set(matrix) != set(REQUIRED_EVIDENCE):
            errors.append("Release readiness must contain exactly the required acceptance items.")
        elif any(type(value) is not bool for value in matrix.values()):
            errors.append("Every release readiness value must be a JSON boolean.")
        else:
            completed = [name for name in REQUIRED_EVIDENCE if matrix[name]]
            pending = [name for name in REQUIRED_EVIDENCE if not matrix[name]]

    versions = {}
    for relative_path, keys in VERSION_SOURCES:
        try:
            value = read_document(root, relative_path)
            for key in keys:
                value = value[key]
        except (OSError, ValueError, KeyError, TypeError):
            errors.append(f"Cannot read release version from {relative_path}.")
            continue
        if not isinstance(value, str) or not re.fullmatch(VERSION_PATTERN, value):
            errors.append(f"Release version in {relative_path} must have the form X.Y.Z.")
        else:
            versions[relative_path] = value

    version = versions.get("package.json")
    if len(set(versions.values())) > 1:
        errors.append("Release versions do not match across package, Tauri and Cargo manifests.")
    validated_tag = None
    if tag is not None:
        if not re.fullmatch("v" + VERSION_PATTERN, tag):
            errors.append("Release tag must have the form vX.Y.Z.")
        else:
            validated_tag = tag
            if version is not None and tag[1:] != version:
                errors.append("Release tag does not match the manifest version.")

    ready_for_production = not errors and not pending
    return {
        "schema_version": 1,
        "channel": channel,
        "version": version,
        "tag": validated_tag,
        "passed": not errors and (channel == "experimental" or ready_for_production),
        "ready_for_production": ready_for_production,
        "completed": completed,
        "pending": pending,
        "errors": errors,
        "warnings": (
            ["Experimental packages are not approved for production."]
            if channel == "experimental" else []
        ) + ["Independent review still applies."],
        "versions": versions,
    }


def summary_markdown(report):
    # Values below are enums, validated version/tag strings, fixed messages or
    # known evidence names. Do not copy unvalidated input into a GitHub summary.
    lines = [
        "## Release evidence",
        "",
        f"Channel: **{report['channel']}**. Gate: **{'passed' if report['passed'] else 'blocked'}**.",
        f"Version: `{report['version'] or 'invalid'}`. Tag: `{report['tag'] or 'not supplied or invalid'}`.",
        f"Production acceptance: **{'complete' if report['ready_for_production'] else 'incomplete'}**.",
        "",
        f"Completed: {len(report['completed'])}. Pending: {len(report['pending'])}.",
    ]
    if report["pending"]:
        lines += ["", "Pending acceptance items:", ""]
        lines += [f"- `{name}`" for name in report["pending"]]
    if report["errors"]:
        lines += ["", "Validation errors:", ""]
        lines += [f"- {error}" for error in report["errors"]]
    lines += ["", *[f"- {warning}" for warning in report["warnings"]], ""]
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", choices=("experimental", "production"), default="production")
    parser.add_argument("--tag", help="Requested tag, matching every release manifest (vX.Y.Z)")
    parser.add_argument("--report", type=Path, help="Write a JSON report, including on gate failure")
    parser.add_argument("--summary", type=Path, help="Append a controlled Markdown report (e.g. GITHUB_STEP_SUMMARY)")
    args = parser.parse_args()
    report = evaluate(Path(__file__).resolve().parents[1], args.channel, args.tag)
    try:
        if args.report:
            args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        if args.summary:
            with args.summary.open("a", encoding="utf-8", newline="\n") as summary:
                summary.write(summary_markdown(report))
    except OSError as error:
        print(f"Cannot write release evidence output: {error}", file=sys.stderr)
        return 1
    for error in report["errors"]:
        print(error, file=sys.stderr)
    if report["passed"]:
        print(f"Release metadata valid for {args.channel}: {report['version']}.")
        if report["pending"]:
            print("Experimental packaging allowed; production acceptance remains incomplete: "
                  + ", ".join(report["pending"]))
        else:
            print("Release evidence gates explicitly complete; independent review still applies.")
    elif args.channel == "production" and report["pending"]:
        print("Production release blocked: " + ", ".join(report["pending"]), file=sys.stderr)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
