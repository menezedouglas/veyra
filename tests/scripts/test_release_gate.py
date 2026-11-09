#!/usr/bin/env python3
"""Exercise the release gate CLI against isolated repository fixtures."""

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/release-gate.py"
SPEC = importlib.util.spec_from_file_location("release_gate", SCRIPT)
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class ReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="veyra-release-gate-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        shutil.copy2(SCRIPT, self.root / "scripts/release-gate.py")
        self.matrix = {name: True for name in GATE.REQUIRED_EVIDENCE}
        self.matrix["windows_installer_verified"] = False
        self.write_matrix()
        for relative, keys in GATE.VERSION_SOURCES:
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if target.suffix == ".toml":
                target.write_text("[" + ".".join(keys[:-1]) + ']\nversion = "0.1.0"\n', encoding="utf-8")
            else:
                target.write_text(json.dumps({"version": "0.1.0"}), encoding="utf-8")

    def write_matrix(self):
        target = self.root / "docs/release-readiness.json"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(self.matrix), encoding="utf-8")

    def run_gate(self, *args):
        return subprocess.run(
            [sys.executable, str(self.root / "scripts/release-gate.py"), *args],
            cwd=self.root, capture_output=True, text=True, encoding="utf-8", check=False,
        )

    def read_report(self):
        return json.loads((self.root / "evidence.json").read_text(encoding="utf-8"))

    def test_default_remains_strict_production(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("Production release blocked: windows_installer_verified", result.stderr)

    def test_experimental_keeps_pending_evidence_in_report_and_summary(self):
        before = (self.root / "docs/release-readiness.json").read_bytes()
        summary = self.root / "summary.md"
        summary.write_text("Earlier job summary\n", encoding="utf-8")
        result = self.run_gate("--channel", "experimental", "--tag", "v0.1.0",
                               "--report", "evidence.json", "--summary", "summary.md")
        self.assertEqual(result.returncode, 0, result.stderr)
        report = self.read_report()
        self.assertTrue(report["passed"])
        self.assertFalse(report["ready_for_production"])
        self.assertEqual(report["pending"], ["windows_installer_verified"])
        self.assertEqual(report["tag"], "v0.1.0")
        self.assertEqual(len(report["versions"]), 5)
        self.assertTrue(report["warnings"])
        self.assertEqual((self.root / "docs/release-readiness.json").read_bytes(), before)
        markdown = summary.read_text(encoding="utf-8")
        self.assertTrue(markdown.startswith("Earlier job summary\n"))
        self.assertIn("`windows_installer_verified`", markdown)
        self.assertIn("Production acceptance: **incomplete**", markdown)

    def test_production_failure_still_writes_evidence(self):
        result = self.run_gate("--channel", "production", "--report", "evidence.json")
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.read_report()["passed"])
        self.assertEqual(self.read_report()["pending"], ["windows_installer_verified"])

    def test_complete_production_evidence_passes(self):
        self.matrix["windows_installer_verified"] = True
        self.write_matrix()
        result = self.run_gate("--tag", "v0.1.0", "--report", "evidence.json")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.read_report()["ready_for_production"])

    def test_both_channels_reject_invalid_checklist_schema(self):
        invalid_matrices = (
            [],
            {},
            {**self.matrix, "windows_installer_verified": "true"},
            {**self.matrix, "windows_installer_verified": 1},
            {**self.matrix, "unknown_acceptance": True},
        )
        for matrix in invalid_matrices:
            self.matrix = matrix
            self.write_matrix()
            for channel in ("experimental", "production"):
                with self.subTest(matrix=matrix, channel=channel):
                    result = self.run_gate("--channel", channel, "--report", "evidence.json")
                    self.assertEqual(result.returncode, 1)
                    self.assertTrue(self.read_report()["errors"])

    def test_invalid_or_duplicate_json_is_not_treated_as_pending(self):
        target = self.root / "docs/release-readiness.json"
        duplicate = json.dumps(self.matrix)[:-1] + ', "windows_installer_verified": true}'
        for invalid in ("{", duplicate):
            target.write_text(invalid, encoding="utf-8")
            result = self.run_gate("--channel", "experimental", "--report", "evidence.json")
            self.assertEqual(result.returncode, 1)
            self.assertIn("invalid or unreadable JSON", result.stderr)

    def test_both_channels_reject_invalid_and_mismatched_tags(self):
        for channel in ("experimental", "production"):
            for tag in ("v0.2.0", "0.1.0", "v01.1.0", "v0.1.0\n## Injected"):
                with self.subTest(channel=channel, tag=tag):
                    result = self.run_gate("--channel", channel, "--tag", tag,
                                           "--report", "evidence.json", "--summary", "summary.md")
                    self.assertEqual(result.returncode, 1)
                    self.assertTrue(self.read_report()["errors"])
        self.assertNotIn("Injected", (self.root / "summary.md").read_text(encoding="utf-8"))

    def test_both_channels_reject_version_mismatch_in_each_manifest(self):
        for relative, _keys in GATE.VERSION_SOURCES:
            target = self.root / relative
            original = target.read_text(encoding="utf-8")
            target.write_text(original.replace("0.1.0", "0.2.0"), encoding="utf-8")
            for channel in ("experimental", "production"):
                with self.subTest(manifest=relative, channel=channel):
                    result = self.run_gate("--channel", channel, "--report", "evidence.json")
                    self.assertEqual(result.returncode, 1)
                    self.assertTrue(self.read_report()["errors"])
            target.write_text(original, encoding="utf-8")

    def test_missing_or_invalid_manifest_is_rejected(self):
        target = self.root / "Cargo.toml"
        for source in ('[workspace.package]\nversion = "nightly"\n', "[", ""):
            target.write_text(source, encoding="utf-8")
            result = self.run_gate("--channel", "experimental", "--report", "evidence.json")
            self.assertEqual(result.returncode, 1)
            self.assertTrue(self.read_report()["errors"])
        target.unlink()
        self.assertEqual(self.run_gate("--channel", "experimental").returncode, 1)


if __name__ == "__main__":
    unittest.main()
