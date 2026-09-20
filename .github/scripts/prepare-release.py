"""Stamp the Android package and lockfile in the disposable release checkout."""
import os
from pathlib import Path
import re
import sys
import tomllib


def prepare_release(root: Path, version: str) -> int:
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("Version must be MAJOR.MINOR.PATCH, without a v prefix")
    parts = tuple(map(int, version.split(".")))
    if any(part > 255 for part in parts) or parts == (0, 0, 0):
        raise ValueError("Each component must be 0..255 and version must exceed 0.0.0")
    # cargo-apk 0.9.7 / ndk-build 0.9.0 uses APK ID 1 and three u8 components.
    code = (1 << 24) | (parts[0] << 16) | (parts[1] << 8) | parts[2]
    manifest = root / "runtimes/oculus_runtime/Cargo.toml"
    lockfile = root / "Cargo.lock"
    original = manifest.read_text()
    old_version = tomllib.loads(original)["package"]["version"]
    stamped, count = re.subn(
        r'(?m)^version = "' + re.escape(old_version) + r'"$',
        f'version = "{version}"', original, count=1,
    )
    locked, lock_count = re.subn(
        r'(\[\[package\]\]\nname = "shock2quest"\nversion = ")'
        + re.escape(old_version) + r'"',
        lambda match: match[1] + version + '"', lockfile.read_text(),
    )
    if count != 1 or lock_count != 1:
        raise ValueError("Expected exactly one matching Android package and lock entry")
    manifest.write_text(stamped)
    lockfile.write_text(locked)
    return code


if __name__ == "__main__":
    version = sys.argv[1]
    code = prepare_release(Path.cwd(), version)
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"version={version}\ntag=v{version}\nversion_code={code}\n")
