import importlib.util
import os
from pathlib import Path
import plistlib
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("package_installer", Path(__file__).with_name("package-installer.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class PackagingTests(unittest.TestCase):
    def test_windows_requires_both_adb_dlls_and_notices(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ["adb.exe", "AdbWinApi.dll", "NOTICE.txt", "game.apk"]:
                (root / name).write_bytes(b"fixture")
            with self.assertRaisesRegex(ValueError, "AdbWinUsbApi.dll"):
                package.payload_files(root / "game.apk", root, True)
            (root / "AdbWinUsbApi.dll").write_bytes(b"dll")
            with zipfile.ZipFile(root / "game.apk", "w") as archive:
                archive.writestr("AndroidManifest.xml", "fixture")
            files = package.payload_files(root / "game.apk", root, True)
            self.assertEqual(set(files), {"adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "NOTICE.txt", "shock2quest.apk"})

    def test_mac_bundle_has_helper_apk_notices_and_correct_entrypoint(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ["adb", "NOTICE.txt", "game.apk", "ui"]:
                (root / name).write_bytes(name.encode())
            with zipfile.ZipFile(root / "game.apk", "w") as archive:
                archive.writestr("AndroidManifest.xml", "fixture")
            files = package.payload_files(root / "game.apk", root, False)
            app = package.mac_app(root / "ui", files, root, "0.2.3")
            with (app / "Contents/Info.plist").open("rb") as f:
                info = plistlib.load(f)
            self.assertEqual(info["CFBundleShortVersionString"], "0.2.3")
            self.assertEqual((app / "Contents/MacOS" / info["CFBundleExecutable"]).read_bytes(), b"ui")
            self.assertEqual((app / "Contents/Helpers/adb").read_bytes(), b"adb")
            self.assertEqual((app / "Contents/Resources/shock2quest.apk").read_bytes(), (root / "game.apk").read_bytes())
            self.assertTrue((app / "Contents/Resources/NOTICE.txt").is_file())
            if os.name != "nt":  # Windows does not preserve Unix executable bits.
                self.assertTrue((app / "Contents/Helpers/adb").stat().st_mode & 0o111)
            with self.assertRaisesRegex(ValueError, "already exists"):
                package.mac_app(root / "ui", files, root, "0.2.3")

    def test_rejects_download_that_is_not_an_apk(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ["adb", "NOTICE.txt", "game.apk"]:
                (root / name).write_bytes(b"html error page")
            with self.assertRaises(zipfile.BadZipFile):
                package.payload_files(root / "game.apk", root, False)
            with zipfile.ZipFile(root / "game.apk", "w") as archive:
                archive.writestr("other.txt", "fixture")
            with self.assertRaisesRegex(ValueError, "missing AndroidManifest"):
                package.payload_files(root / "game.apk", root, False)


if __name__ == "__main__":
    unittest.main()
