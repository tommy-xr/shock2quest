# dark_vr_tool

`cargo dvr` opens a live Quest dashboard in a terminal. With redirected input or
output it runs `status` instead. Device commands require `adb` on PATH; Android builds need
the existing Oculus SDK/signing setup described in `DEVELOPMENT.md`.

```sh
cargo dvr devices
cargo dvr --serial SERIAL status     # or export ANDROID_SERIAL=SERIAL
cargo dvr mission get
cargo dvr mission set medsci1.mis
cargo dvr mission set debug_gloves
cargo dvr mission unset             # same as: mission set ""
cargo dvr build                     # release APK; no ADB or headset needed
cargo dvr run                       # release build, install, launch, follow logs
cargo dvr run --no-logcat           # return after launch
cargo dvr deploy                    # release build + install; does not launch
cargo dvr deploy --apk /path/to/shock2quest.apk
cargo dvr pr 1234                   # install + launch PR #1234's CI APK (gh)
cargo dvr pr 1234 --mission debug_gloves --no-uninstall
cargo dvr assets copy /path/to/remaster
cargo dvr assets recopy /path/to/remaster  # delete old assets, then copy
cargo dvr assets delete             # removes retail assets, including custom mods
cargo dvr wifi                      # USB -> ADB over Wi-Fi until reboot, then unplug
cargo dvr launch                    # restart installed app, no build
cargo dvr stop
cargo dvr ls                        # defaults to /sdcard/shock2quest
cargo dvr ls res                    # relative to /sdcard/shock2quest
cargo dvr ls saves                  # saved games (*.sav), once a save exists
cargo dvr ls /sdcard/shock2quest/mods
cargo dvr shell                     # interactive device shell
cargo dvr shell 'df -h /sdcard'      # explicit remote shell syntax
cargo dvr logs
cargo dvr recordings               # input recordings, newest first
cargo dvr pull-recording            # newest recording + its start save -> ./recordings
cargo dvr pull-recording rec-1727390000000 --out /tmp/recs
cargo dvr debug-port set 8171
cargo dvr debug-port get
cargo dvr debug-port unset
```

`wifi` needs the headset on USB once: it reads its `wlan0` address, runs
`adb tcpip` (`--port`, default 5555) and connects. The headset stays reachable through sleep, but
a reboot returns it to USB-only. While USB and Wi-Fi are both attached, the tool
matches transports by hardware serial and prefers USB. Pass `--serial IP:PORT`
to force Wi-Fi. Different headsets still require an explicit selection.

`pr` installs the APK CI built for the PR's head commit (in-game label
`pr | <sha7>`). CI signs it with a dedicated dev key (`CN=Shock2Quest Dev (CI)`,
not the release key). Local and CI builds share the checked-in version 0.0.0,
below every release, so neither installs as an update over a release.

Sign local builds with the same dev key and `pr` replaces them in place: get the
keystore and password from the owner and point cargo-apk at them,
`CARGO_APK_RELEASE_KEYSTORE` / `CARGO_APK_RELEASE_KEYSTORE_PASSWORD` for
`cargo dvr build`/`run`/`deploy` (release profile), or `CARGO_APK_DEV_KEYSTORE` /
`CARGO_APK_DEV_KEYSTORE_PASSWORD` for a plain `cargo apk build`. An install
signed with another key, or versioned above 0.0.0 (a release, or a build from
before the version reset), is uninstalled first; `--no-uninstall` refuses
instead. Saves live under `/sdcard/shock2quest`, so an uninstall keeps them.
Builds that kept saves in app-private storage (releases v0.0.3/v0.0.4, 0.1.0 dev
builds) are never uninstalled by the tool, nor is one whose version cannot be
read. A later release installed over v0.0.3/v0.0.4 in place moves their saves
on first launch; 0.1.0 builds have no in-place path, so uninstalling one (by
hand) deletes its saves.

`assets copy` transfers `sshock2.kpf`, `mods/`, and `cutscenes/` into
`/sdcard/shock2quest`, merging existing directories. `assets recopy` deletes those
three paths first, including any custom mods. Both accept `DARK_ASSET_PATH` when
the source argument is omitted and require all three source entries before doing
any device work. These commands stop the app; launch it again after copying.
An interrupted recopy can leave incomplete assets; rerun it to finish.
Saves, recordings, settings, legacy data and other files are preserved.
Repository `assets/` resources are bundled into the APK and updated by deployment.

The dashboard refreshes every five seconds: **m** edits the mission, **l** launches,
**s** stops, **f** lists game files, **r** returns to status, and **q** quits.
Press Enter on an empty mission field to delete the override; Escape cancels.
ADB work runs off the UI thread, so a slow device does not block Quit.

Input recordings (Developer > Camera & view > **Rec input R-stick**, then click the
right stick) are stored in `/sdcard/shock2quest/recordings` as `rec-<ms>.jsonl` plus
the `.sav` they start from. `pull-recording` copies both and prints the command that
renders the recording to a GIF/MP4.

Saved games are stored in `/sdcard/shock2quest/saves/*.sav`. The `saves`
directory may not exist until a game has been saved.

Mission configuration writes `/sdcard/shock2quest/vr-mission.txt`, the actual
runtime startup override (not `vr_mission.txt` or a separate `vr_override` file).
`vr-override` is an alias for the `mission` command. Missing configuration boots
`main_menu`. Names are validated using the runtime's parser, but the tool does not
verify that a mission exists in your installed game archives. Changes apply at
next app launch. A provisioned `benchmark-scene.json` takes precedence over the
mission override. An unreadable or invalid benchmark configuration opens the main
menu with "Benchmark scene failed to load"; `logs` includes the detailed
`SHOCK2QUEST_BENCHMARK_ERROR`. Inspect or remove it with the shell command:

```sh
ss2vr shell 'cat /sdcard/shock2quest/benchmark-scene.json'
ss2vr shell 'rm -f /sdcard/shock2quest/benchmark-scene.json'
ss2vr mission unset  # also clear the mission override to boot the main menu
```

These commands use the `ss2vr` alias described below; `cargo dvr` also works.
Removing the benchmark file does not clear the separate mission override.

`debug-port` writes `/sdcard/shock2quest/debug-port.txt`. Restart the app after
changing it, then use `adb -s SERIAL forward tcp:8171 tcp:8171` to reach its
loopback debug server. Unsetting the file disables the server on next launch.
Other gameplay settings remain in the in-game Developer options panel.

`build`, `run` and `deploy` (except `deploy --apk`) source `runtimes/oculus_runtime/set_up_android_sdk.sh`
from that directory and invoke `cargo apk build --release`. They print the APK
path beneath Cargo's configured target directory. `run` and `deploy` only select
a device after the build succeeds, so you can connect the headset during a build.
`run` installs, restarts the app, and follows the same filtered logcat as `logs`;
Ctrl-C exits the command. Launching an activity does not establish
that the headset is focused or rendering: this tool does not force proximity or
Guardian state. Shell commands deliberately support arbitrary remote shell code;
`ls` paths and configuration values are treated as literal data.

For the requested shell shorthand, add this to your `~/.zshrc` (or `~/.bashrc`):

```sh
alias ss2vr='cargo dvr'
# If you already have alias c=cargo, c dvr works too.
```

Run the alias from this checkout. Plain status requires exactly one authorized
headset unless `--serial` or `ANDROID_SERIAL` selects one. USB and Wi-Fi transports
with the same hardware serial count as one headset; missing or conflicting
identities require explicit selection. `devices` also shows offline and
unauthorized transports.

Validation: `cargo test -p dark_vr_tool` covers parsing, device selection, terminal
layout, and command behavior with an isolated fake ADB (Python 3 required for
those Unix integration tests). No physical headset is required for this suite.
