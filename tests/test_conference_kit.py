import hashlib
import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("conference_kit", Path(__file__).parents[1] / "scripts/package-conference-kit.py")
kit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kit)


class ConferenceKitTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "source"
        self.root.mkdir()
        for name in kit.SOURCE_DIRS:
            (self.root / name).mkdir(parents=True)
        for name in kit.ROOT_FILES:
            (self.root / name).write_text("source fixture")
        (self.root / "config/conference-dependencies.json").write_text('{"rvoip":{"version":"0.3.12"}}')
        for name in ["CONTRIBUTING_CONNECTORS.md", "UCTP_CONFERENCE_PROFILE.md", "CONFERENCE_RUNBOOK.md", "CONFERENCE_VOICE_REHEARSAL.md", "CONFERENCE_IMPLEMENTATION_STATUS.md"]:
            (self.root / "docs" / name).write_text("fixture documentation")
        (self.root / "clients/uctp-js").mkdir()
        (self.root / "clients/uctp-js/client.mjs").write_text("// reference client")
        (self.root / "web/conference-kit").mkdir()
        (self.root / "web/conference-kit/index.html").write_text("<!doctype html>")
        self.out = Path(self.temp.name) / "kit"
        mock = patch.object(kit.subprocess, "check_output", return_value="test-base-commit")
        mock.start()
        self.addCleanup(mock.stop)

    def test_archive_excludes_private_workspace_and_matches_manifest(self):
        (self.root / "var").mkdir()
        (self.root / "var/credentials.json").write_text("private-credential-1234")
        (self.root / ".env").write_text("private-credential-1234")
        (self.root / "participant-recording.wav").write_bytes(b"private audio")
        result = kit.build(self.root, self.out, ["private-credential-1234"])
        self.assertEqual((self.out / "CONFERENCE_VOICE_REHEARSAL.md").read_text(), "fixture documentation")
        with tarfile.open(self.out / "parley-conference-kit.tar.gz") as archive:
            names = archive.getnames()
            self.assertFalse(any("credentials" in name or name.endswith(".env") or name.endswith(".wav") for name in names))
            manifest = json.load(archive.extractfile("parley/KIT_MANIFEST.json"))
            for name, digest in manifest["files"].items():
                self.assertEqual(hashlib.sha256(archive.extractfile("parley/" + name).read()).hexdigest(), digest)
            self.assertEqual(set(names), {"parley/" + name for name in manifest["files"]} | {"parley/KIT_MANIFEST.json"})
        second = kit.build(self.root, Path(self.temp.name) / "second", [])
        self.assertEqual(result["archive_sha256"], second["archive_sha256"])
        with self.assertRaisesRegex(ValueError, "overwrite"):
            kit.build(self.root, self.out)

    def test_known_credential_in_source_fails_before_output(self):
        (self.root / "config/default.toml").write_text('key="private-credential-1234"')
        with self.assertRaisesRegex(ValueError, "credential found"):
            kit.build(self.root, self.out, ["private-credential-1234"])
        self.assertFalse(self.out.exists())

    def test_unexpected_files_and_symlinks_are_rejected(self):
        unexpected = self.root / "config/credentials.pem"
        unexpected.write_text("private key")
        with self.assertRaisesRegex(ValueError, "Unexpected"):
            kit.build(self.root, self.out)
        unexpected.unlink()
        (self.root / "src/private.rs").symlink_to(self.root / ".env.example")
        with self.assertRaisesRegex(ValueError, "symlink"):
            kit.build(self.root, self.out)


if __name__ == "__main__":
    unittest.main()
