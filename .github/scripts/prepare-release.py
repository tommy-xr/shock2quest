"""Stamp the Android package and lockfile in the disposable release checkout."""
import argparse
import json
import os
from pathlib import Path
import re
import tomllib


def parse_release_version(version: str) -> tuple[int, int, int]:
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", version):
        raise ValueError("Version must be MAJOR.MINOR.PATCH, without a v prefix")
    parts = tuple(map(int, version.split(".")))
    if any(part > 255 for part in parts) or parts == (0, 0, 0):
        raise ValueError("Each component must be 0..255 and version must exceed 0.0.0")
    return parts


def check_published_releases(version: str, releases: list[dict]) -> None:
    requested = parse_release_version(version)
    for release in releases:
        tag = release["tag_name"]
        if tag == f"v{version}":
            raise ValueError(f"Release already exists: {tag} (including drafts)")
        if release["draft"]:
            continue
        try:
            published = parse_release_version(tag.removeprefix("v"))
        except ValueError:
            continue  # Ignore tags outside this pipeline's version scheme.
        if requested <= published:
            raise ValueError(f"Version {version} must exceed published release {tag}")


def prepare_release(root: Path, version: str) -> int:
    parts = parse_release_version(version)
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
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--published-releases", type=Path,
                        help="Validate against gh api --paginate --slurp release pages; do not stamp")
    args = parser.parse_args()
    if args.published_releases:
        pages = json.loads(args.published_releases.read_text())
        check_published_releases(args.version, [release for page in pages for release in page])
    else:
        code = prepare_release(Path.cwd(), args.version)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"version={args.version}\ntag=v{args.version}\nversion_code={code}\n")
