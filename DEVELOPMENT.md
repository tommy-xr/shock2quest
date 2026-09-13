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

shock2quest reads an unmodified **25th Anniversary Remaster** install. You need
two things from it:

- `sshock2.kpf` — the base game data.
- the `mods/` folder — the remaster's upgraded models and textures, which the
  VR hands and weapons are built against.

Skip `sshock2ee-vault.kpf` (a bonus gallery) and the root `sshock2ee.kpf`
(frontend-only) — nothing reads either, and together they are ~1.5 GB.

Either copy those into the repo, or point the engine at your install.

**Option A — copy into `Data/`**

- Copy the files above into the `shock2quest/Data` folder

**Option B — set `DARK_ASSET_PATH`**

Point the engine at game files that live outside the repo (handy when sharing
one copy of the data across several clones):

```bash
export DARK_ASSET_PATH=/path/to/your/shock2/data
```

**Either way, the data directory must contain a _sentinel_ file** —
`sshock2.kpf`. This is how the engine recognizes a directory as game data.
(`paths.rs` also accepts `shock2.gam`, `res/obj.crf`, `res/mesh.crf` and
`motiondb.bin`, which is how a pre-remaster install is still picked up. That
path is legacy and is missing the upgraded VR art.)

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

### Debug & developer keys

Player-facing controls are listed in [README.md](README.md#controls). The keys
below are development tools; they are bound in
`runtimes/desktop_runtime/src/input_mapper.rs` (the authoritative list) and, on
the Quest, in `InputAction::quest_touch_click_path` /
`quest_touch_chord_paths` (`shock2vr/src/input/actions.rs`).

Debug bindings take `Alt` (`Option` on macOS) to keep them clear of gameplay keys.

| Key | Action | Notes |
| --- | ------ | ----- |
| `P` | `PathfindingTestCycle` | set start → set goal → show path |
| `B` | `DebugCycleWeapon` | spawns and wields the next weapon (unlike the number row) |
| `Alt+X` | `EjectClip` | magazine back to the backpack reserve; no Quest button - in VR it is the settings MFD's UNLOAD |
| `T` / `Y` | `CycleAmmo` / `CyclePsiPower` | no Quest binding: a clip is inserted by hand, and the psi MFD's stick navigation steps the power |
| - | `SelectPsiPower` | opens the psi power selection MFD; flat clicks the readout's power badge instead, Quest: the amp hand's *upper* face button |
| `F` | `CycleGunSetting` | switch the wielded gun's fire mode (e.g. NORM / BURST); Quest: the gun hand's *upper* face button |
| `U` | `ReadLastUnreadLog` | on Quest a free hand's *upper* face button resolves to this - see below |
| `M` | `ToggleMap` | flat only |
| `Tab` / `I` | `ToggleUseMode` | the cyber interface; Quest: a **short** press of left `Menu` |
| `Space` | `Jump` | the flat key is the held jump channel, not this action; Quest: either *lower* face button |
| `Esc` | `TogglePauseMenu` | Quest: left `Menu` held ~0.5 s, or the interface's MENU button |
| `Alt+S` / `Alt+L` | `QuickSave` / `QuickLoad` | |
| `Alt+G` | `DebugForceChase` | every monster hunts the player, pinned |
| `Alt+C` | `DebugCalmAll` | clears the pin |
| `Alt+V` | `ToggleFreeCamera` | requires the **Free camera** developer option; Quest: right `A`+`B` together |

Every action is also triggerable without a keyboard - over HTTP on the debug
runtime (`POST /v1/input/action`) or through the SDK (`game.input.trigger(...)`).
`GET /v1/input/actions` lists them.

#### The cyber interface carries the use-mode readouts

The expanded bio (BIOFULL) and ammo (AMMOFULL) readouts along the bottom of the
640x480 interface canvas are part of the interface itself, not the flat HUD -
one emit (`shock2vr/src/hud/readouts.rs`) drawn by the pointer host, so the VR
cyber-interface panel shows the same pair at the same pixels the flat cursor
clicks. Their SETTING / RELOAD / ammo-cycle / psi-selector controls are
hit-tested from that same layout, so the weapon settings and psi power MFDs open
from the readout with the controller ray exactly as they do with the mouse.
`GET /v1/ui` reports everything they draw as `readout_elements` (derived by
replaying the same emit, so it cannot drift) and their controls as `readout`, in
both presentations. Outside use mode flat keeps its compact
BIO/AMMOBACK overlay and VR its forearm panels.

The VR **forearm** panels are not suppressed while the interface is up, so in
VR use mode the same bio and ammo readings appear both on the arms and on the
interface - deliberately left alone for now (issue #1268), unlike flat, which
does drop its compact pair.

#### Quest face buttons are per-hand and contextual

The four face buttons are bound raw, by hand and position - lower is left `X` /
right `A`, upper is left `Y` / right `B` (`LeftHandLowerButton` ..
`RightHandUpperButton`). What a press does is resolved per hand against what
that hand holds (`shock2vr/src/hand_buttons.rs`):

| that hand holds | lower (`X`/`A`) | upper (`Y`/`B`) |
| --- | --- | --- |
| nothing, a melee weapon, or any other item | `Jump` | `ReadLastUnreadLog` |
| a gun | `Jump` | tap/release switches that gun's fire mode; hold ~0.5 s drops its loaded clip, with progress on its cuff meter |
| an ammo clip | `Jump` | swap with the next compatible carried ammo type, returning the original clip to the backpack |
| the psi amp | `Jump` | `SelectPsiPower` - the power selection MFD, in the cyber interface |

The lower button is jump unconditionally: it is the one control a player reaches
for with both hands full, so a held weapon must not take it away. Only the upper
button is contextual. The settings MFD's UNLOAD and `EjectClip` still return rounds to the backpack;
upper-button holding drops a physical clip instead. `CyclePsiPower` stays on
the psi MFD's stick navigation. Both actions keep their flat keys and HTTP/SDK paths.

Gun taps fire on release, and a completed hold fires once and swallows release.
Changing weapons, entering a panel, pausing, death, or a session interruption
cancels a pending hold. Empty and energy weapons eject nothing. Ammo swaps
preserve the actual clip entities and counts, skip unavailable or unrelated
ammo, and refuse if the outgoing clip cannot fit in the backpack. The other
hand's gun determines compatibility first, then carried guns, then authored
ammo families when no gun is carried.

Mode first: while the cyber interface is up the lower button keeps its close
(rather than jumping) and the upper one the log reader, on both hands whatever
is held, so the interface can always be shut. While the **Free camera**
developer option is on, the right hand's two buttons are the chord and nothing
else - they resolve to nothing at all.

#### The left Menu button: short press jacks in, long press pauses

The Touch has one Menu button to give (the right one is the Quest system UI's),
so it is bound raw (`MenuButton`) and split by how long it is held
(`shock2vr/src/input/menu_hold.rs`):

- **short press** (released under 0.5 s) - toggle the cyber interface, which is
  what the left `X` button used to do. It fires on *release*, so a long press
  never also jacks in on its way past the threshold.
- **long press** (held 0.5 s) - open the pause menu, fired the moment the
  threshold is crossed. The release that follows is swallowed.

While the button is held, the same circular hold readout the cutscene skip uses
fills up head-locked in front of the player, so the pause menu is a visible
promise rather than a surprise. Because the hold is not discoverable on its own,
the cyber interface canvas also carries a **MENU** button (rect `(288, 372,
64x36)`, in the free column between the inventory strip, the MFD slot and the
bottom readouts) that opens the pause menu directly - drawn and hit-tested
through the same readout-control list, so it is there in both presentations
(flat's `Esc` still works too).

#### The psi selection MFD captures a thumbstick

While the psi power selection MFD is docked - opened from the flat readout's
power badge, or from the amp hand's upper face button in VR - **one** thumbstick
is captured: up/down step the tier, left/right step the power inside it, and
that stick stops driving the player until the panel closes.

Which stick is the one the player is not already using to hold or aim the
weapon, so it differs by presentation:

| | captured | still drives |
| --- | --- | --- |
| flat | the **left** stick (arrow-key turn) | `WASD` still walks |
| VR | the stick of the hand **not** holding the amp | the amp hand keeps aiming |

Steps are edge-triggered, so a held stick moves one place, and they are the same
`StepPsiSelection` the readout's four arrows emit - the selection applies live,
and the panel pages to whatever tier it lands on.

### Campaign difficulty

New Game offers Easy, Normal (the default), Hard, and Impossible. Choose a level,
then Start Game; the choice is fixed for that campaign. Starting another game
creates a fresh character and fresh mission state.

For debug runs, use `cargo dbgr --mission medsci1.mis --difficulty hard`.
The choices are `easy`, `normal` (default), `hard`, and `impossible`; the SDK
accepts the same strings as `GameServer.launch({ mission, difficulty })`.
Difficulty is fixed for a campaign and reported by `/v1/info` as
`player.difficulty`. Saves and deck transitions retain it, including when loading
an existing campaign from a runtime launched with a different choice. The generic
quest-bit API cannot change it. The choice drives authored player pools, trainer
and replicator prices, mission object masks, and Easy ecology and hypo bonuses.

### Developer options

The **Developer** screen (from the main menu, or the pause overlay's Developer
page) hosts the live-tunable parameters registered in
`shock2vr/src/dev_params.rs` - panel distance, pause dim, FOV override, and the
free-camera switches. Values are read every frame, so a change is live on the
next one, and the same registry is exposed over HTTP by the debug runtime
(`GET`/`POST /v1/dev-params`) for headless runs.

#### Testing VR weapon handling at different stats

Open `debug_weapons` from the Developer scene list and pick up a gun from the
bench. Hold the left Menu button to pause, then open **Developer**. The
**Gun STR ovrd** and **Gun AGI ovrd** rows override Strength and Agility for
physical gun handling: `0` follows your character, and `1`–`6` selects a test
level. Use the row arrows, return to the game, and repeat while holding the
same weapon. Decrease both back to `0` to restore character-driven handling.

The pen starts with maxed character stats, so try STR `1`, `3`, `6` with AGI
`1` first; compare one hand with the fore-end supported. Then hold STR at `3`
and vary AGI `1`, `3`, `6`. Strength reduces recoil and downward muzzle weight;
Agility reduces angular recoil (the extra one-handed pitch remains at AGI 6).
These knobs do not change weapon skill, shot spread, inventory capacity,
movement, or your saved character sheet. They remain active across scene/load
changes in the running process and reset on app restart. Hand-motion inertia
is not implemented yet.

**Back scale**, **Pitch scale**, and **Yaw scale** independently
multiply new recoil impulses on each axis, for both one-handed and supported
shots. Each ranges from `0` to `3`, defaults to `1`, and uses `0.1` steps. `0`
disables new recoil on that axis; existing recoil settles normally. Authored
travel limits and recovery rates still apply, so high gains can reach the caps.
These scales do not change downward weight or restore pitch/yaw suppressed by
Agility or Still Hand. For a first balance experiment, try back `0.3`, pitch `1.5`,
yaw `1`, with STR `1` and AGI `1`; these are test values, not new defaults.
**1-hand scale** multiplies only the extra one-handed recoil after Strength:
`0` removes that penalty, `1` keeps the current amount, and up to `3` increases
it. The axis gains still apply to both baseline and extra recoil; supported
shots retain baseline recoil regardless of this setting. Downward weight is
separate. Return all four scales to `1` to restore the profiles.

Quest enables physical held guns, downward gun weight, and particles by default.
Desktop/debug VR testing needs
`--vr --experimental physical_held_items,physical_gun_weight`. Automated tests
can set the same knobs through `game.devParams.set("gun_strength_override", 3)`
and `game.devParams.set("gun_agility_override", 1)`; `reset(key)` restores `0`.

**Free camera** detaches the view from the player: the camera stays where it
was while the pawn stands still, so you can watch the simulation from outside
without perturbing it. Nothing in the simulation follows it - AI keeps reading
the player's body position - and by default neither does culling, so flying out
shows exactly what the *player's* viewpoint decided to draw. Turn on **Cull
from cam** when you would rather just look at things. The developer option is
the gate: while it is off the `Alt+V` / `A`+`B` toggle does nothing, and turning
it off while detached re-attaches the camera.

Flying uses the ordinary locomotion controls, so they are the ones your hands
already know - and while the camera has them, the player stands still rather
than sleepwalking off:

| Control | Desktop | Effect |
| ------- | ------- | ------ |
| Right thumbstick | `W` `A` `S` `D` | fly along the view / strafe |
| Left thumbstick x | `←` `→` | turn |
| Left thumbstick y | `↑` `↓` | rise / fall |
| Head / mouse | mouse | aim (the camera is a freeze-frame of the eye, so looking works as it always does) |
| — | `Shift` | double speed |

The camera noclips - flying through a wall to look at the far side is the tool
working, not a bug. **Cam speed** sets the pace in the player's own speed
units - the default *is* the player's walk speed. Those are pre-scale SS2
units, not world units per second (the default 25 travels 10 world units a
second), because the camera divides by `dark::SCALE_FACTOR` exactly as
locomotion does.

Headlessly, `GET /v1/camera` reports whether the camera is detached and the
pose it is rendering from, so a test can check what the camera did without
reading pixels - see `tools/shock2-sdk/test/free-camera.e2e.test.ts`.

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

- Create a `develop.keystore` for signing release APKs. Keep it **outside** the
  repo - it is gitignored, so a copy inside one clone is invisible to every other
  clone and worktree:
  ```sh
  mkdir -p ~/.shock2quest
  keytool -genkey -v -keystore ~/.shock2quest/develop.keystore \
    -alias com_tommybuilds_shock2quest -keyalg RSA -keysize 2048 -validity 10000
  ```
  - The password must match `keystore_password` in
    `runtimes/oculus_runtime/Cargo.toml`.
  - `set_up_android_sdk.sh` symlinks it into whichever checkout you source it
    from, so each new clone or worktree picks it up with no extra step. Set
    `SHOCK2QUEST_KEYSTORE` to keep it somewhere else.
- Make sure [Developer Mode is enabled on your Quest device](https://www.reddit.com/r/OculusQuest/comments/17sa8n6/tutorial_quest_3_developer_mode_4_easy_steps/)
- Make sure `adb` is installed and working. With Oculus connected, run `adb devices` and verify your headset shows up
- Tweak `runtimes/oculus_runtime/set_up_android_sdk.sh` to match your paths
- Before running for the first time, you'll need to copy over the System Shock 2
  data files. From your install directory (~1.2 GB):
  ```sh
  adb shell mkdir -p /sdcard/shock2quest/mods
  adb push sshock2.kpf /sdcard/shock2quest/
  for f in sshock2ee 400 shtup scp patch_ext; do
    adb push "mods/$f.kpf" /sdcard/shock2quest/mods/
  done
  ```
  The runtime switches to the remaster as soon as `sshock2.kpf` is present, and
  does not mount the legacy `.crf` archives at all in that mode — so pushing
  these over an older install is safe and needs no cleanup first.

##### Running

- `cd runtimes/oculus_runtime`
- `source ./set_up_android_sdk.sh`
- `cargo apk run --release`

**Note**: Cargo aliases (dr, dq, dv) work for desktop development but not for Android builds, which require the full cargo apk commands.

##### Launcher icon

`runtimes/oculus_runtime/res/mipmap/icon.png` is a neutral placeholder, on
purpose: the obvious icon is SHODAN, and that art belongs to the game's
rightsholders, so it is not committed here. Anyone who owns the game can swap in
the 25th Anniversary portrait locally — it is a 1024x1024 BC7 texture with no
logo or watermark, which is what makes it read at launcher-tile size. Run it
from the repo root, with `DARK_ASSET_PATH` pointing at your install:

```sh
pip install texture2ddecoder pillow
python3 - <<'EOF'
import zipfile, os, texture2ddecoder
from PIL import Image
kpf = os.path.join(os.environ["DARK_ASSET_PATH"], "mods/sshock2ee.kpf")
dds = zipfile.ZipFile(kpf).read("obj/txt16/ND-shodan.dds")
W = H = 1024                                  # DX10 header is 20 bytes past the 128-byte DDS header
px = texture2ddecoder.decode_bc7(dds[148:148 + W * H], W, H)
im = Image.frombytes("RGBA", (W, H), px, "raw", "BGRA").convert("RGB")
m = int(W * 0.05)                             # trim the framing so the face fills the tile
im.crop((m, m, W - m, H - m)).resize((512, 512), Image.LANCZOS).save(
    "runtimes/oculus_runtime/res/mipmap/icon.png", optimize=True)
EOF
```

That leaves `res/mipmap/icon.png` modified in your working tree — keep it local,
don't commit it.

##### Wireless deploy & logs (no cable)

Everything in the Quest loop (`cargo apk run`, `adb install`, `adb logcat`) goes
through adb, so it all works over Wi-Fi once adb is connected wirelessly. One-time
setup per headset boot, with the cable plugged in:

```sh
adb tcpip 5555
adb shell ip route     # note the headset's IP, e.g. 192.168.1.42
# unplug the cable, then:
adb connect 192.168.1.42:5555
```

After that, `adb devices` lists `192.168.1.42:5555` and `cargo apk run --release`
installs + launches over the air; `adb logcat` streams logs wirelessly.

- If both a USB and a wireless device are listed, target one with
  `adb -s 192.168.1.42:5555 ...` or `export ANDROID_SERIAL=192.168.1.42:5555`.
- A reboot (and sometimes sleep / a Wi-Fi drop) resets tcpip mode — reconnect the
  cable and repeat. For a fully cable-free flow, use the headset's Wireless
  Debugging (Settings → System → Developer) with `adb pair` once and
  `adb connect` per session, or let Meta Quest Developer Hub manage the
  connection (it auto-reconnects and can keep the headset awake).
- APK push is slower over Wi-Fi than USB; fine for the normal iterate loop.
