# Publishing releases

## Roadmap

- **v0.0.1:** signed Quest APK, installation instructions, checksums, release badge.
- **v0.0.2:** versioned `dark_explorer`, `dark_runtime`, and `dark_query` archives
  across Linux, Windows, and macOS, with `--version` verification.
- **v0.0.3:** automatic changelog listing all commits since the previous release.

The initial workflow uses static release notes; it does not generate a changelog.

## One-time signing setup

Use a permanent release keystore with exactly one private-key entry and matching
key/store passwords (cargo-apk 0.9.7 uses those defaults). Keep an offline backup
of both the keystore and password. Losing the key prevents updates to existing
installations. Do not use the repository's development signing password for a
new public key.

To create a dedicated key, run this interactively outside the repository:

```sh
mkdir -p ~/.shock2quest
keytool -genkeypair -keystore ~/.shock2quest/release.keystore \
  -storetype PKCS12 -alias shock2quest-release -keyalg RSA -keysize 3072 \
  -validity 10000
```

Configure repository Actions secrets (password is prompted, never committed):

```sh
base64 < ~/.shock2quest/release.keystore | tr -d '\n' | \
  gh secret set ANDROID_RELEASE_KEYSTORE_BASE64 --repo tommy-xr/shock2quest
gh secret set ANDROID_RELEASE_KEYSTORE_PASSWORD --repo tommy-xr/shock2quest
```

To preserve upgrades from an existing build, use its keystore instead. A build
signed with a different key requires uninstalling the old app first. Existing
0.1.0 development builds also have a higher version code than release 0.0.1.

## Run a release

1. Merge the release workflow and intended changes to `main`. Manually dispatched
   workflows must be present on the default branch; the release job only runs on
   `main`, where signing secrets are allowed.
2. Open **Actions → Release → Run workflow**, select `main`, and enter `0.0.1`.
   Equivalent CLI:

   ```sh
   gh workflow run release.yml --ref main -f version=0.0.1
   ```

3. Watch the run. It stamps the Android crate and matching Cargo.lock entry in
   its disposable checkout, builds with locked dependencies, verifies signature,
   alignment, package ID, versions, ABI, and non-debuggable status, then uploads
   the checked payload. Only the publish job has repository write permission.
4. On success, `v0.0.1` points to the workflow's exact source commit and the
   published release contains `shock2quest-0.0.1.apk`, `INSTALL.md`, `SHA256SUMS`.
   The README badge and latest-release link update automatically. Releases are
   ordinary GitHub releases, described as early development builds in the notes.
5. Install the first downloaded APK on a Quest and check launch, game-data
   detection, and controls. CI's package checks do not establish playability.

Use increasing MAJOR.MINOR.PATCH versions, with each component in 0..255 and no
leading zeroes, suffixes, or `v` prefix. cargo-apk 0.9.7 encodes versionCode as
`(1 << 24) | (major << 16) | (minor << 8) | patch`; 0.0.1 is 16777217. The same
input sets Android versionName and the Rust package version. The source tag
retains the development manifest; rerun the stamping script with the release
version to reproduce release metadata.

Concurrent release runs are serialized. Existing tags/releases (including drafts)
are rejected, and published artifacts are never overwritten. If publication
fails after creating a draft, inspect it and its attached files before removing
that unpublished draft and any associated tag, then rerun. Never remove a
published tag/release to reuse a version. A build failure before publication
creates no release and can simply be rerun.

The build uses Rust 1.98.0, cargo-apk 0.9.7, NDK 24.0.8215888, Android platform
26 and Build Tools 33.0.0. SDK/FFmpeg setup is shared with ordinary Android CI in
`.github/actions/setup-android/action.yml`.

## Local checks

```sh
python3 -m unittest discover -s .github/scripts -p 'test_*.py'
actionlint .github/workflows/release.yml .github/workflows/build-android.yml
```
