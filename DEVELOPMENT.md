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

- Copy local game files (\*.mis, res folder) into the `shock2quest/Data` folder

### 3. Build Locally

#### 3a. Desktop (Windows, OSX)

> **NOTE:** In theory, this should work on Linux as well - just haven' tried.

##### Pre-requisites

- Install [rust toolchain](https://www.rust-lang.org/tools/install)
- (Windows) Install [cmake](https://cmake.org/install/)

##### Running

- `cd runtimes/desktop_runtime`
- `cargo run --release`

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
- Before running for the first time, you'll need to copy over the system shock 2 data files.
  - From the root of the repo, run: `adb push Data/ /sdcard/shock2quest`

##### Running

- `cd runtimes/oculus_runtime`
- `source ./set_up_android_sdk.sh`
- `cargo apk run --release`
