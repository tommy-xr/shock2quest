# dark_vr_tool

`cargo dvr` opens a live Quest dashboard in a terminal. With redirected input or
output it runs `status` instead. Requires `adb` on PATH; Android builds also need
the existing Oculus SDK/signing setup described in `DEVELOPMENT.md`.

```sh
cargo dvr devices
cargo dvr --serial SERIAL status     # or export ANDROID_SERIAL=SERIAL
cargo dvr mission get
cargo dvr mission set medsci1.mis
cargo dvr mission set debug_gloves
cargo dvr mission unset             # same as: mission set ""
cargo dvr run                       # release build, install, launch, follow logs
cargo dvr run --no-logcat           # return after launch
cargo dvr deploy                    # release build + install; does not launch
cargo dvr deploy --apk /path/to/shock2quest.apk
cargo dvr launch                    # restart installed app, no build
cargo dvr stop
cargo dvr ls                        # defaults to /sdcard/shock2quest
cargo dvr ls res                    # relative to /sdcard/shock2quest
cargo dvr ls saves                  # saved games (*.sav), once a save exists
cargo dvr ls /sdcard/shock2quest/mods
cargo dvr shell                     # interactive device shell
cargo dvr shell 'df -h /sdcard'      # explicit remote shell syntax
cargo dvr logs
cargo dvr debug-port set 8171
cargo dvr debug-port get
cargo dvr debug-port unset
```

The dashboard refreshes every five seconds: **m** edits the mission, **l** launches,
**s** stops, **f** lists game files, **r** returns to status, and **q** quits.
Press Enter on an empty mission field to delete the override; Escape cancels.
ADB work runs off the UI thread, so a slow device does not block Quit.

Saved games are stored in `/sdcard/shock2quest/saves/*.sav`. The `saves`
directory may not exist until a game has been saved.

Mission configuration writes `/sdcard/shock2quest/vr-mission.txt`, the actual
runtime startup override (not `vr_mission.txt` or a separate `vr_override` file).
`vr-override` is an alias for the `mission` command. Missing configuration boots
`main_menu`. Names are validated using the runtime's parser, but the tool does not
verify that a mission exists in your installed game archives. Changes apply at
next app launch. A provisioned `benchmark-scene.json` may select a benchmark
workload independently; remove it manually when finished benchmarking.

`debug-port` writes `/sdcard/shock2quest/debug-port.txt`. Restart the app after
changing it, then use `adb -s SERIAL forward tcp:8171 tcp:8171` to reach its
loopback debug server. Unsetting the file disables the server on next launch.
Other gameplay settings remain in the in-game Developer options panel.

`run` and `deploy` source `runtimes/oculus_runtime/set_up_android_sdk.sh` from that
directory and invoke `cargo apk` with `--release --device SERIAL`. `deploy` resolves
the APK beneath Cargo's configured target directory. `run` follows cargo-apk's
logcat output; Ctrl-C exits the command. Launching an activity does not establish
that the headset is focused or rendering: this tool does not force proximity or
Guardian state. Shell commands deliberately support arbitrary remote shell code;
`ls` paths and configuration values are treated as literal data.

For the requested shell shorthand, add this to your `~/.zshrc` (or `~/.bashrc`):

```sh
alias ss2vr='cargo dvr'
# If you already have alias c=cargo, c dvr works too.
```

Run the alias from this checkout. Plain status requires exactly one authorized
device unless `--serial` or `ANDROID_SERIAL` selects one; it never guesses among
multiple devices. `devices` also shows offline and unauthorized transports.

Validation: `cargo test -p dark_vr_tool` covers parsing, device selection, terminal
layout, and command behavior with an isolated fake ADB (Python 3 required for
those Unix integration tests). No physical headset is required for this suite.
