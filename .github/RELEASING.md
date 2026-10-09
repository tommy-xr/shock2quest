# Publishing releases

## Roadmap

- **v0.0.1:** signed Quest APK, installation instructions, checksums, release badge.
- **v0.0.2:** versioned `dark_explorer`, `dark_runtime`, and `dark_query` archives
  across Linux, Windows, and macOS, with `--version` verification.

The release workflow appends a complete commit changelog with associated merged
PR links. It uses the latest published, non-prerelease ancestor as its baseline
and the exact source commit being built as its endpoint. A separate `notes` job
runs on both build-only and publish runs, tests the release scripts, and uploads
`changelog.md` and `sources.json` as the **release-notes** Actions artifact
(retained for 14 days). Publication waits for both APK and notes jobs and appends
that generated changelog to the release's installation notes.

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
   Leave **publish** unchecked (the default) to test the signed build without
   creating a tag or release. Equivalent CLI:

   ```sh
   gh workflow run release.yml --ref main -f version=0.0.1 -F publish=false
   ```

3. Watch the run. It stamps the Android crate and matching Cargo.lock entry in
   its disposable checkout, checks dependency-lock freshness, builds, verifies signature,
   alignment, package ID, versions, ABI, and non-debuggable status, then uploads
   the checked payload as the **release-apk** Actions artifact (retained for 14
   days). Download and extract it from the run's summary page.
4. Install that APK on a Quest using `INSTALL.md` and check launch, game-data
   detection, cutscene playback, and controls. CI's package checks do not
   establish playability. Build-only runs can reuse a version, including one
   with an existing tag/release.
5. To publish, run the workflow again with **publish** checked:

   ```sh
   gh workflow run release.yml --ref main -f version=0.0.1 -F publish=true
   ```

   This rebuilds from the selected `main` commit; check that it is the commit
   you tested. Only this mode rejects existing tags/releases and versions older
   than any published release, and runs the publish job, which is the only job
   with repository write permission.
   On success, `v0.0.1` points to that exact source commit and the published
   release contains `shock2quest.apk`, `INSTALL.md`, and `SHA256SUMS`.
   The README badge and latest-release link update automatically. Releases are
   ordinary GitHub releases, described as early development builds in the notes.

Every release uses the asset name `shock2quest.apk`, so the stable download URL is
`https://github.com/tommy-xr/shock2quest/releases/latest/download/shock2quest.apk`.
The APK still embeds its release version; the filename does not determine it.

Use increasing MAJOR.MINOR.PATCH versions, with each component in 0..255 and no
leading zeroes, suffixes, or `v` prefix. cargo-apk 0.9.7 encodes versionCode as
`(1 << 24) | (major << 16) | (minor << 8) | patch`; 0.0.1 is 16777217. The same
input sets Android versionName and the Rust package version. The source tag
retains the development manifest; rerun the stamping script with the release
version to reproduce release metadata.

Concurrent release runs are serialized. When publishing, existing tags/releases
(including drafts) are rejected, and published artifacts are never overwritten. If publication
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

## Changelog and visual highlights

Preview the next release locally with authenticated `gh` and a full checkout
(fetch tags/history first if your clone is shallow):

```sh
python3 .github/scripts/release-notes.py --since v0.0.1 --until HEAD
```

This writes `target/release-notes/changelog.md` and `sources.json`. Every commit
in the exact range is retained, including direct commits and merge commits;
PR associations come from GitHub, not commit-title guesses. PR bodies are saved
once per PR as source material for image curation. API errors abort generation
rather than producing a silently incomplete changelog. Omit `--since` to use the
same automatic baseline as CI; `--output` selects a different output directory.
Generation only reads Git/GitHub and writes local files; it never publishes.

Next, invoke the [release-highlights skill](../.claude/skills/release-highlights/SKILL.md)
to inspect existing PR media, select up to three player-facing visual highlights,
and write `highlights.md`, `selection.md`, and `release-description.md` beside
the changelog. Before/after captions refer to each PR's baseline unless verified
against the prior release. Image selection is editorial and stays outside CI.

The publish workflow appends only the mechanical changelog. Add the reviewed
visual highlights to the release description separately, preserving its install
instructions and provenance. Local previews can be rerun as more PRs land; rerun
curation for the final release SHA so the highlights match the shipped range.
