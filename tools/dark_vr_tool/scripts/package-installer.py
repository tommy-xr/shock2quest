#!/usr/bin/env python3
"""Build a portable Windows installer or a macOS app/DMG from supplied payloads."""
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import zipfile

REPO = Path(__file__).resolve().parents[3]


def payload_files(apk, tools, windows):
    names = ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"] if windows else ["adb"]
    files = {name: tools / name for name in names}
    files.update({"NOTICE.txt": tools / "NOTICE.txt", "shock2quest.apk": apk})
    for name, path in files.items():
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Missing or empty {name}: {path}")
    with zipfile.ZipFile(apk) as archive:
        if "AndroidManifest.xml" not in archive.namelist():
            raise ValueError(f"Not an APK (missing AndroidManifest.xml): {apk}")
    return files


def build_binary(bundle_dir, debug):
    env = os.environ.copy()
    env.pop("DARKVR_BUNDLE_DIR", None)
    if bundle_dir:
        env["DARKVR_BUNDLE_DIR"] = str(bundle_dir)
    args = ["cargo", "build", "--locked", "-p", "dark_vr_tool", "--features", "installer",
            "--message-format=json-render-diagnostics"]
    if not debug:
        args.append("--release")
    executable = None
    with subprocess.Popen(args, cwd=REPO, env=env, stdout=subprocess.PIPE, text=True) as proc:
        for line in proc.stdout:
            message = json.loads(line)
            if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == "dark_vr_tool":
                executable = message.get("executable") or executable
        if proc.wait():
            raise RuntimeError("Installer build failed")
    if not executable:
        raise RuntimeError("Cargo did not report the installer executable")
    return Path(executable)


def mac_app(binary, files, output, version):
    app = output / "Shock2Quest Installer.app"
    if app.exists():
        raise ValueError(f"Output already exists: {app}")
    contents = app / "Contents"
    for directory in ["MacOS", "Helpers", "Resources"]:
        (contents / directory).mkdir(parents=True)
    shutil.copy2(binary, contents / "MacOS/shock2quest-installer")
    shutil.copy2(files["adb"], contents / "Helpers/adb")
    for name in ["shock2quest.apk", "NOTICE.txt"]:
        shutil.copy2(files[name], contents / "Resources" / name)
    for path in [contents / "MacOS/shock2quest-installer", contents / "Helpers/adb"]:
        path.chmod(0o755)
    with (contents / "Info.plist").open("wb") as f:
        plistlib.dump({
            "CFBundleName": "Shock2Quest Installer",
            "CFBundleDisplayName": "Shock2Quest Installer",
            "CFBundleIdentifier": "com.tommybuilds.shock2quest.installer",
            "CFBundleExecutable": "shock2quest-installer",
            "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version,
            "CFBundleVersion": version,
            "NSHighResolutionCapable": True,
        }, f)
    return app


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", required=True, type=Path)
    parser.add_argument("--platform-tools", required=True, type=Path,
                        help="Extracted Android platform-tools for the target OS, including NOTICE.txt")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--version", default="0.1.0")
    parser.add_argument("--debug", action="store_true", help="Use the development Cargo profile")
    parser.add_argument("--binary", type=Path, help="Use an existing installer-feature binary (macOS only)")
    parser.add_argument("--sign-identity", help="macOS Developer ID Application identity; omit for a local unsigned build")
    parser.add_argument("--notary-profile", help="macOS notarytool keychain profile (requires --sign-identity)")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if not windows and sys.platform != "darwin":
        parser.error("Package on Windows or macOS using that OS's platform-tools")
    if windows and (args.binary or args.sign_identity or args.notary_profile):
        parser.error("--binary, --sign-identity and --notary-profile are macOS-only")
    if args.notary_profile and not args.sign_identity:
        parser.error("--notary-profile requires --sign-identity")
    files = payload_files(args.apk.resolve(), args.platform_tools.resolve(), windows)
    args.out.mkdir(parents=True, exist_ok=True)
    output = args.out.resolve()
    arch = platform.machine().lower().replace("amd64", "x64").replace("x86_64", "x64").replace("aarch64", "arm64")
    if windows:
        with tempfile.TemporaryDirectory(prefix="darkvr-payload-") as temp:
            payload = Path(temp)
            for name, source in files.items():
                shutil.copy2(source, payload / name)
            binary = build_binary(payload, args.debug)
            artifact = output / f"Shock2Quest-Installer-windows-{arch}.exe"
            shutil.copy2(binary, artifact)
    else:
        binary = args.binary.resolve() if args.binary else build_binary(None, args.debug)
        subprocess.run([str(binary), "ui", "--help"], check=True, stdout=subprocess.DEVNULL)
        app = mac_app(binary, files, output, args.version)
        if args.sign_identity:
            for path in [app / "Contents/Helpers/adb", app / "Contents/MacOS/shock2quest-installer", app]:
                subprocess.run(["codesign", "--force", "--options", "runtime", "--timestamp", "--sign", args.sign_identity, str(path)], check=True)
        artifact = output / f"Shock2Quest-Installer-macos-{arch}.dmg"
        with tempfile.TemporaryDirectory(prefix="darkvr-dmg-") as temp:
            staging = Path(temp)
            shutil.copytree(app, staging / app.name)
            (staging / "Applications").symlink_to("/Applications")
            subprocess.run(["hdiutil", "create", "-volname", "Shock2Quest Installer", "-srcfolder", str(staging),
                            "-format", "UDZO", str(artifact)], check=True)
        if args.notary_profile:
            subprocess.run(["xcrun", "notarytool", "submit", str(artifact), "--keychain-profile", args.notary_profile, "--wait"], check=True)
            subprocess.run(["xcrun", "stapler", "staple", str(artifact)], check=True)
    print(artifact)


if __name__ == "__main__":
    main()
