# ss2-remake

A project to experience the awesomesess of System Shock 2 in virtual reality. System Shock 2 is one of my favorite games of all time, and I believe the story and ambience would be a great for VR. 

This project is a [game engine recreation](https://en.wikipedia.org/wiki/Game_engine_recreation) of the [Dark engine](https://en.wikipedia.org/wiki/Dark_Engine) - geared towards VR experiences.

You'll need a full retail copy of System Shock 2 in order to play - recommend purchasing at either [GoG](https://www.gog.com/game/system_shock_2) or [Steam](https://store.steampowered.com/app/238210/System_Shock_2/). It's often on sale at one or the other for a couple bucks, and totally worth it.

## Installation

## Development

### Desktop

#### Pre-requisites
- Install [rust toolchain]
- (Windows) Install [cmake]

- `cd desktop_runtime`
- `cargo run --release`

### Oculus Quest 2

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

## Roadmap

__Pre Alpha__: Initial development
    - [x] Load gamesys
        - [x] Speech DB / env sounds 
    - [ ] Load/save
    - [ ] Basic item usage
    - [ ] Initial inventory management
    - [ ] Initial pass on psi powers
    - [ ] Basic AI
    - [ ] Act/React implementation
    - [ ] Lighting implementation
- __Alpha__: Playable end-to-end, with bugs and missing features 
    - Playable end-to-end
    - Parity with vanilla SS2, with the exception of tweaks for VR
- __Beta__: Parity with vanilla SS2, with the exception of tweaks for VR
    - Mod support
        - DML file support
- __Release__: 
    - [ ] Multiplayer
    - [ ] Horde / rogue-like mode (ie, Prey:Mooncrash style)?

## License

Some code is ported from [openDarkEngine](https://github.com/volca02/openDarkEngine), so this code is licensed under [GPLv2](https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html) to comply with that license.

In addition, code in the `engine` folder is dual-licensed under the MIT license.

## Inspiration

There were several projects around the SS2 community, that served as an inspiration, or were used to help understand the internals of the dark engine and file formats:

- [openDarkEngine](https://github.com/volca02/openDarkEngine)
- [SystemShock2VR](https://github.com/Kernvirus/SystemShock2VR)

Outside of the system shock/thief community, the work that [Team Beef](https://sidequestvr.com/community/7/team-beef-game-ports) has done in bringing games to VR inspired this project. 

# Special Thanks

First off, thank you to [Nightdive Studios](https://www.nightdivestudios.com/) for keeping these retro games alive, as well as Le Corbeau for thew NewDark patches that allowed me to revisit the SS2 universe :)






