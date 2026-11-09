#!/usr/bin/env python3
"""Exercise release assembly through its CLI using isolated unsigned fixtures."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class PrepareReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="veyra-prepare-release-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        shutil.copy2(ROOT / "scripts/prepare-release.py", self.root / "scripts/prepare-release.py")
        shutil.copy2(ROOT / "package.json", self.root / "package.json")
        self.version = json.loads((self.root / "package.json").read_text(encoding="utf-8"))["version"]
        self.artifacts = self.root / "release-artifacts"
        self.linux = self.artifacts / "Linux/deb" / f"Veyra_{self.version}_amd64.deb"
        self.windows = self.artifacts / "Windows/nsis" / f"Veyra_{self.version}_x64-setup.exe"
        for path, data in ((self.linux, b"synthetic deb\x00\xff"), (self.windows, b"synthetic exe\x00\xfe")):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.report = {
            "schema_version": 1,
            "channel": "experimental",
            "version": self.version,
            "tag": "v" + self.version,
            "passed": True,
            "ready_for_production": False,
            "pending": ["windows_installer_verified", "windows_native_backend_packaged"],
            "errors": [],
        }
        self.write_evidence()

    def write_evidence(self):
        (self.root / "release-evidence.json").write_text(json.dumps(self.report, indent=2) + "\n", encoding="utf-8")

    def run_prepare(self):
        return subprocess.run(
            [sys.executable, str(self.root / "scripts/prepare-release.py"),
             "--artifacts", "release-artifacts", "--evidence", "release-evidence.json",
             "--output", "release-files", "--notes", "release-notes.md"],
            cwd=self.root, capture_output=True, text=True, encoding="utf-8", check=False,
        )

    def assert_rejected_without_output(self, message):
        result = self.run_prepare()
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertFalse((self.root / "release-files").exists())
        self.assertFalse((self.root / "release-notes.md").exists())

    def test_both_installers_and_original_evidence_have_verified_checksums(self):
        result = self.run_prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.root / "release-files"
        expected = {path.name: path.read_bytes() for path in (self.linux, self.windows, self.root / "release-evidence.json")}
        self.assertEqual({path.name for path in output.iterdir()}, {*expected, "SHA256SUMS"})
        checksums = (output / "SHA256SUMS").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(checksums), 3)
        self.assertEqual({line.split("  ", 1)[1] for line in checksums}, set(expected))
        for line in checksums:
            digest, name = line.split("  ", 1)
            self.assertEqual((output / name).read_bytes(), expected[name])
            self.assertEqual(digest, hashlib.sha256(expected[name]).hexdigest())
        notes = (self.root / "release-notes.md").read_text(encoding="utf-8")
        self.assertIn("**experimental**", notes)
        self.assertIn("**unsigned**", notes)
        self.assertIn("not approved for production", notes)
        self.assertIn("No independent audit is claimed", notes)
        for name in self.report["pending"]:
            self.assertIn(f"`{name}`", notes)

    def test_missing_either_platform_is_rejected(self):
        for path in (self.linux, self.windows):
            original = path.read_bytes()
            path.unlink()
            with self.subTest(missing=path.suffix):
                self.assert_rejected_without_output("one Linux .deb and one Windows")
            path.write_bytes(original)

    def test_duplicate_basenames_are_rejected(self):
        duplicate = self.artifacts / "extra" / self.linux.name
        duplicate.parent.mkdir()
        duplicate.write_bytes(b"different stale build")
        self.assert_rejected_without_output("Duplicate installer asset basename")

    def test_stale_or_foreign_installer_version_is_rejected(self):
        for path in (self.linux, self.windows):
            renamed = path.with_name(path.name.replace(self.version, "9.9.9"))
            path.rename(renamed)
            with self.subTest(platform=path.suffix):
                self.assert_rejected_without_output("Installer filename must identify Veyra and the release version")
            renamed.rename(path)

    def test_failed_gate_and_wrong_report_metadata_are_rejected(self):
        original = self.report.copy()
        for changes, message in (
            ({"passed": False}, "did not pass"),
            ({"passed": 1}, "did not pass"),
            ({"schema_version": True}, "schema_version 1"),
            ({"channel": "stable"}, "channel must be"),
            ({"tag": "v9.9.9"}, "version and tag must match"),
            ({"version": "9.9.9"}, "version and tag must match"),
            ({"errors": ["wrong manifest"]}, "validation errors"),
            ({"pending": ["bad\n## Markdown"]}, "unique acceptance names"),
            ({"pending": ["same", "same"]}, "unique acceptance names"),
            ({"ready_for_production": 1}, "boolean ready_for_production"),
            ({"ready_for_production": True}, "contradicts its pending"),
        ):
            self.report = {**original, **changes}
            self.write_evidence()
            with self.subTest(changes=changes):
                self.assert_rejected_without_output(message)

    def test_invalid_or_duplicate_evidence_json_is_rejected(self):
        evidence = self.root / "release-evidence.json"
        for content in ("{", "[]", json.dumps(self.report)[:-1] + ', "passed": true}'):
            evidence.write_text(content, encoding="utf-8")
            result = self.run_prepare()
            self.assertEqual(result.returncode, 1)
            self.assertFalse((self.root / "release-files").exists())
            self.assertNotIn("Traceback", result.stderr)

    def test_production_requires_complete_acceptance(self):
        self.report["channel"] = "production"
        self.write_evidence()
        self.assert_rejected_without_output("not ready for production")
        self.report.update(ready_for_production=True, pending=[])
        self.write_evidence()
        result = self.run_prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        notes = (self.root / "release-notes.md").read_text(encoding="utf-8")
        self.assertIn("Production acceptance: **complete**", notes)
        self.assertNotIn("not approved for production", notes)

    def test_existing_output_is_preserved_and_rejected(self):
        output = self.root / "release-files"
        output.mkdir()
        stale = output / "stale.exe"
        stale.write_bytes(b"keep existing bytes")
        result = self.run_prepare()
        self.assertEqual(result.returncode, 1)
        self.assertIn("output directory already exists", result.stderr)
        self.assertEqual(list(output.iterdir()), [stale])
        self.assertEqual(stale.read_bytes(), b"keep existing bytes")

    def test_existing_notes_are_preserved_and_rejected(self):
        notes = self.root / "release-notes.md"
        notes.write_text("reviewer's notes\n", encoding="utf-8")
        result = self.run_prepare()
        self.assertEqual(result.returncode, 1)
        self.assertIn("notes file already exists", result.stderr)
        self.assertFalse((self.root / "release-files").exists())
        self.assertEqual(notes.read_text(encoding="utf-8"), "reviewer's notes\n")


if __name__ == "__main__":
    unittest.main()
