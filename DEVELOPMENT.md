# Development

## Source Code Overview

- `shock2quest`
  - `dark` - module that reads dark engine file formats (bin, mis, cal, gam, etc)
  - `engine` - core OpenGL rendering engine
  - `references` - just some output that was useful to refer to (ie, text form of the namemaps)
  - `runtimes`
    - `desktop_runtime` - code for running the desktop version
    - `tool` - a tool for viewing models and experimentation outside of gameplay
    - `oculus_runtime` - runtime for oculus using OpenXR
  - `shock2vr` - core gameplay logic
    - `scripts` - implementation of all the scripts needed for objects
    - `mission` - core logic for running a mission
    - `save_load` - serializing, deserializing game state
    - `creature` - constants and hitboxes for creature definitions

## Set up

### 1. Clone Repoo

- `git clone https://github.com/tommybuilds/shock2quest`
- `cd shock2quest`

### 2. Provide data files

shock2quest reads an unmodified retail install. Two layouts are supported:

- **25th Anniversary Remaster (recommended)** — the data lives inside KPF
  archives. You need `sshock2.kpf` and the `mods/` folder. The remaster's
  upgraded models and textures are what the VR hand and weapon work is built
  against. (`sshock2ee-vault.kpf` is a bonus gallery and the root
  `sshock2ee.kpf` is frontend-only — neither is read; skip them.)
- **Classic (1999) install** — loose `*.mis` files plus the `res/` folder of
  `.crf` archives. Still supported, but missing the upgraded VR art.

Either copy the files into the repo, or point the engine at an existing copy.

**Option A — copy into `Data/`**

- Copy the files above into the `shock2quest/Data` folder

**Option B — set `DARK_ASSET_PATH`**

Point the engine at game files that live outside the repo (handy when sharing
one copy of the data across several clones):

```bash
export DARK_ASSET_PATH=/path/to/your/shock2/data
```

**Either way, the data directory must contain at least one _sentinel_ file** —
`sshock2.kpf` (remaster), or `shock2.gam`, `res/obj.crf`, `res/mesh.crf`, or
`motiondb.bin` (classic). This is how the engine recognizes a directory as game
data.

> **Gotcha:** if `DARK_ASSET_PATH` is set but contains no sentinel, it does *not*
> fail. It logs a warning and falls back to searching `./Data`, `../Data`,
> `../../Data`, `.` — which typically surfaces later as a confusing
> `shock2.gam not found`, even though the variable looks correctly set. If you
> hit that, check the sentinels before anything else.

Resolution order lives in `shock2vr/src/paths.rs`.

### 2b. Enable git hooks (recommended)

The repo ships a pre-push hook that runs `cargo fmt --all -- --check` so
unformatted Rust never reaches a PR (it would otherwise fail the CI "Format
Check"). Enable it once per clone:

```bash
git config core.hooksPath .githooks
```

The hook only checks formatting before a push (it does not slow down individual
commits). Bypass in a pinch with `git push --no-verify`.

### 3. Build Locally

#### 3a. Desktop (Windows, OSX)

> **NOTE:** In theory, this should work on Linux as well - just haven' tried.

##### Pre-requisites

- Install [rust toolchain](https://www.rust-lang.org/tools/install)
- (Windows) Install [cmake](https://cmake.org/install/)

##### Running

- `cd runtimes/desktop_runtime`
- `cargo run --release`

##### Quick Start with Cargo Aliases

Alternatively, use the project's cargo aliases from the root directory:
- `cargo dr --release` - Run desktop runtime
- `cargo dq entities --help` - Use dark_query CLI tool
- `cargo dv --help` - Use dark_viewer tool

Example:
```bash
cargo dr --release --experimental teleport
```

#### 3b. Oculus Quest 2

##### Pre-requisites

- Install Android SDK

  - Mac:
    - Install Java 8: https://stackoverflow.com/a/46405092
    - Install Android SDK: https://guides.codepath.com/android/installing-android-sdk-tools
    - Install tools
      - `sdkmanager "build-tools;33.0.0"`
      - `sdkmanager "platform-tools" "platforms;android-26"`
      - `sdkmanager "ndk;24.0.8215888"`
      - `sdkmanager --update`
    - Install cargo-apk: `cargo install cargo-apk`
    - Add android target: `rustup target add aarch64-linux-android`
    - Install adb: `brew install android-platform-tools`

- Create a `develop.keystore`. (ie, https://stackoverflow.com/questions/25975320/create-android-keystory-private-key-command-line)
  - Make sure the password matches in the Cargo.toml file:
  ```sh
  keytool -genkey -v -keystore develop.keystore -alias com_tommybuilds_shock2quest  -keyalg RSA -keysize 2048 -validity 10000
  ```
- Make sure [Developer Mode is enabled on your Quest device](https://www.reddit.com/r/OculusQuest/comments/17sa8n6/tutorial_quest_3_developer_mode_4_easy_steps/)
- Make sure `adb` is installed and working. With Oculus connected, run `adb devices` and verify your headset shows up
- Tweak `runtimes/oculus_runtime/set_up_android_sdk.sh` to match your paths
- Before running for the first time, you'll need to copy over the System Shock 2 data files.
  - **Remaster (recommended)** — from your install directory, ~1.2 GB:
    ```sh
    adb shell mkdir -p /sdcard/shock2quest/mods
    adb push sshock2.kpf /sdcard/shock2quest/
    for f in sshock2ee 400 shtup scp patch_ext; do
      adb push "mods/$f.kpf" /sdcard/shock2quest/mods/
    done
    ```
  - **Classic** — from the root of the repo: `adb push Data/ /sdcard/shock2quest`

  The two can coexist on the device: the runtime switches to the remaster as
  soon as `sshock2.kpf` is present, and does not mount the `.crf` archives at
  all in that mode. Deleting the KPFs reverts to the classic install without
  re-pushing it.

##### Running

- `cd runtimes/oculus_runtime`
- `source ./set_up_android_sdk.sh`
- `cargo apk run --release`

**Note**: Cargo aliases (dr, dq, dv) work for desktop development but not for Android builds, which require the full cargo apk commands.
