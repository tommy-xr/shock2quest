
# Development

## 1. Clone Repoo

- `git clone https://github.com/tommybuilds/shock2quest`
- `cd shock2quest`

## 2. Provide data files

- Copy local game files (*.mis, res folder) into the `shock2quest/Data` folder

## 3. Build Locally

## 3a. Desktop (Windows, OSX)

> __NOTE:__ In theory, this should work on Linux as well - just haven' tried.

#### Pre-requisites
- Install [rust toolchain](https://www.rust-lang.org/tools/install)
- (Windows) Install [cmake](https://cmake.org/install/)

### Running

- `cd runtimes/desktop_runtime`
- `cargo run --release`

## 3b. Oculus Quest 2

#### Pre-requisites
- Install Android SDK
    - Mac: 
        - Install Java 8: https://stackoverflow.com/a/46405092
        - Install Android SDK: https://guides.codepath.com/android/installing-android-sdk-tools
        - Install tools
            - `sdkmanager "build-tools;33.0.0"`
            - `sdkmanager "platforms;android-26"`
            - `sdkmanager "platform-tools" "platforms;android-26"`
            - `sdkmanager "ndk;24.0.8215888"`
            - `sdkmanager --update`
        - rustup target add aarch64-linux-android
        - Install adb: `brew install android-platform-tools`

- Create a `develop.keystore`. (ie, https://stackoverflow.com/questions/25975320/create-android-keystory-private-key-command-line)
    - Make sure the password matches in the Cargo.toml file: 
    ```
    keytool -genkey -v -keystore develop.keystore -alias com_tommybuilds_shock2quest  -keyalg RSA -keysize 2048 -validity 10000
    ```

- `source ./common.sh`
- `cd oculus_runtime`
- `cargo apk run --release`