import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest

spec = importlib.util.spec_from_file_location(
    "prepare_release", Path(__file__).with_name("prepare-release.py")
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
ROOT = Path(__file__).resolve().parents[2]


class ReleaseVersionTests(unittest.TestCase):
    def test_publication_requires_numeric_increase_over_every_release(self):
        published = [{"tag_name": "v0.2.0", "draft": False},
                     {"tag_name": "v0.1.9", "draft": False}]
        for version in ("0.1.1", "0.2.0"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.check_published_releases(version, published)
        release.check_published_releases("0.10.0", published)
        release.check_published_releases("1.0.0", published)
        release.check_published_releases("0.0.1", [])

    def test_drafts_reserve_only_their_own_version(self):
        drafts = [{"tag_name": "v1.0.0", "draft": True}]
        release.check_published_releases("0.0.1", drafts)
        with self.assertRaises(ValueError):
            release.check_published_releases("1.0.0", drafts)

    def test_unrelated_tags_do_not_prevent_publication(self):
        release.check_published_releases("0.0.1", [
            {"tag_name": "nightly", "draft": False},
        ])

    def checkout(self, root):
        for name in ("runtimes/oculus_runtime/Cargo.toml", "Cargo.lock"):
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((ROOT / name).read_bytes())

    def test_stamps_only_android_package_and_preserves_lock_dependencies(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.checkout(root)
            before = tomllib.loads((root / "Cargo.lock").read_text())
            code = release.prepare_release(root, "0.0.1")
            self.assertEqual(code, 16777217)
            manifest = tomllib.loads((root / "runtimes/oculus_runtime/Cargo.toml").read_text())
            self.assertEqual(manifest["package"]["version"], "0.0.1")
            after = tomllib.loads((root / "Cargo.lock").read_text())
            for package in before["package"]:
                if package["name"] == "shock2quest":
                    package["version"] = "0.0.1"
            self.assertEqual(before, after)
            self.assertEqual(release.prepare_release(root, "0.0.1"), code)
            self.assertGreater(release.prepare_release(root, "0.1.0"), code)
            self.assertGreater(release.prepare_release(root, "1.0.0"), 16777472)

    def test_rejects_invalid_versions_without_touching_files(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for version in ("", "v0.0.1", "0.0.0", "01.0.0", "1.2", "1.2.3-rc1",
                            "1.2.3\n", "256.0.0", "0.256.0", "0.0.256", "$(id)"):
                with self.subTest(version=version), self.assertRaises(ValueError):
                    release.prepare_release(root, version)
            self.assertEqual(list(root.iterdir()), [])

    def test_lock_mismatch_fails_before_manifest_write(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.checkout(root)
            manifest = root / "runtimes/oculus_runtime/Cargo.toml"
            before = manifest.read_bytes()
            (root / "Cargo.lock").write_text("")
            with self.assertRaises(ValueError):
                release.prepare_release(root, "0.0.1")
            self.assertEqual(manifest.read_bytes(), before)
