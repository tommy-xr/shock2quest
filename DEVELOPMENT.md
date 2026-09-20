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

## Releases

See [Publishing releases](.github/RELEASING.md) for signing setup and the manual
release workflow. Users installing an APK should follow [INSTALL.md](INSTALL.md).

## Set up

### 1. Clone Repoo

- `git clone https://github.com/tommybuilds/shock2quest`
- `cd shock2quest`

### 2. Provide data files

shock2quest reads an unmodified **25th Anniversary Remaster** install. Copy these from it:

- `sshock2.kpf` — the base game data.
- the `mods/` folder — the remaster's upgraded models and textures, which the
  VR hands and weapons are built against.
- the `cutscenes/` folder, with its subfolders intact — the game's videos.

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
cargo run --release -p desktop_runtime -- --vr --experimental physical_held_items
```

### Song explorer

Open `cargo dx ui --select song/engsong.snc` (or select a `.snc` in the
explorer's song family or Archives tab). **Play song** sends the authored start
event; **Stop** cuts off the current WAV immediately. Event buttons queue the
latest event for the next clip boundary. `theme ...` events persist across clip
boundaries until another theme is selected; other events are one-shot. Restart
clears the previous theme and selects the authored start event.

The view lists section WAVs, event branches and normalized branch probabilities.
Green marks the playing section; blue marks the branch the player actually took.
Recent transitions show the requested event, matched/default option and selected
weight. **Audition WAV** stops the song and plays that sample alone. Changing
assets or tabs stops playback.

For native-window capture, `--play-song --screenshot /tmp/song.png
--screenshot-after 1` starts the selected song and captures after one second.
Playback uses the audio device clock; these captures are not fixed-timestep.
The installed-asset regression can be run with
`cargo test -p dark_explorer mounted_songs -- --ignored`.

### Gameplay music themes

Mission music is driven by `PropAmbientHacked` markers with the `MUSIC` flag.
Entering a marker's radius selects its schema as a theme: `quiet` becomes
`theme quiet`, `restart` becomes `theme restart`, etc. The shared song player
retains that theme after leaving the marker and reapplies it at each WAV
boundary, falling back to the section's default branch when unhandled. A new
song starts fresh; standing inside a marker supplies its theme again after a
level load. Events match complete names, case-insensitively (`quiet` and
`quietlo`, or `begin` and `begin2`, are distinct).

For example, MedSci 1 object 2014 (`music quiet turret1`) selects `quiet`;
2026 selects `restart`, and 2013 selects `end`. These are location triggers,
not automatic reactions to AI alertness or combat. Their musical effects come
from each `.snc` graph: not every song supports every theme. The installed
songs also use `quietlo`, `soft`, `bass`, `windy`, `beet`, `break`, `begin2`,
and `begin3`. An unhandled event follows the default branch, not a guessed
musical equivalent. Horde has no authored spatial theme markers and continues
using each wave song's start theme; rest/preparation stop playback.

The persistence behavior follows the original `sound/ambient.c` →
`SongUtilSetTheme` → `cSongPlayer::SetTheme` / `_DoSegmentCallback` path.
Sample offsets, branch randomness, and themes outside markers are not saved.

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
| `Tab` | `ToggleUseMode` | the cyber interface; Quest: a **short** press of left `Menu` |
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

#### Left and right weapon readouts

The bottom-right ammo panel carries **LEFT** and **RIGHT** selectors above its
native weapon controls. Both show their hand's loaded rounds, or `PSI` for an
amp; `--` means the hand has no ammo readout and cannot be selected. A cyan
underline marks the weapon whose ammo type, condition, settings, and reload
controls are shown below. Clicking a selector does not equip or fire anything.
The inventory's hand slots still select the same readout.

Selection follows the weapon if it changes hands. Dropping the selected weapon
falls back to the other gun or amp. Switching weapons dismisses the old gun's
settings MFD, while the character MFD remains open. Both presentations use the
same tab and control rectangles; `/v1/ui.readout` exposes `select_left_hand` and
`select_right_hand`, each with its own weapon entity ID. Empty hands are
non-clickable and appear only in `readout_elements`.

#### Character MFD and access cards

The card icon beside the log button opens the collected access-card reader.
It lists the keyring's named access regions; the cards are credentials, not
inventory items. The adjacent **MFD** button opens the character sheet, with
**STATS**, **TECH**, **CMBT**, and **PSI** tabs in the original right-hand panel.
Stats and skills use arrow meters; Tech also shows installed software versions.
Point at a stat, skill, software icon, trait, or psi discipline for its authored
description. The Psi tab browses all five tiers and distinguishes trained powers
using the same icon grid as the amp selector. Character-sheet browsing is
read-only and does not change the amp's selected power or browsed tier.

These controls share their layout in flat and VR. `GET /v1/ui` exposes their
buttons and current contents in `utilities`, including `access_cards`,
`character_stats`, `character_tab_0` through `character_tab_3`, and `psi_tier_N`.

#### Query item information

Select **?** in the cyber interface to arm the inspection cursor, then select
an inventory item or a held-item readout. The left QUERY panel shows its short
name and authored description without using, moving, or consuming the item.
Selection returns the cursor to normal; select **?** again to cancel before
selecting anything. An item already being dragged keeps ownership of the cursor.
The four arrow buttons scroll by line or page, and the top-right X closes the
reader. Unresearched items show the original research-required message until
identified. The preview currently uses the inventory icon, not the original
rotating 3D model.

Flat and VR use the same canvas, selection logic, and text bounds. Debug UI labels
include `inspect`, `query_title`, `query_line_up`, `query_line_down`,
`utility_previous`, `utility_next`, and `utility_close`.

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

While a mission is running, **Pause → Developer → Cheats** includes
**Add radiation (+10)**, **Add toxin (+10)**, and **Clear radiation + toxin**.
Repeated clicks add another 10 points, even with protective equipment equipped.
Clear resets both stored exposure levels to zero; environmental hazards can
expose you again after resuming. These cheats work in flatscreen and VR.

The **Developer** screen (from the main menu, or the pause overlay's Developer
page) hosts the live-tunable parameters registered in
`shock2vr/src/dev_params.rs` - panel distance, pause dim, FOV override, and the
free-camera switches. Values are read every frame, so a change is live on the
next one, and the same registry is exposed over HTTP by the debug runtime
(`GET`/`POST /v1/dev-params`) for headless runs.

**Camera & view → Ambient intensity** (`ambient_light_intensity`) scales the
mission's authored world ambient floor and the fixed `0.5` ambient contribution
on object and creature materials. It defaults to `1`, ranges from `0` to `3`
in `0.05` steps, and resets on app restart. `0.5` halves those contributions;
`0` removes them. Emissive contributions and runtime spotlights remain independent.
UI, video, and explicitly fullbright world surfaces keep their existing brightness.

**Camera & view → Level light intensity** (`level_light_intensity`) scales baked
world lightmaps, also from `0` to `3` in `0.05` steps with default `1`.
The ambient floor still applies after this scaling; lower both controls to
darken both baked lighting and the minimum light level. Objects do not yet
receive authored level lights on this branch, so this control currently affects
only world lightmaps. When per-object level lighting lands (#1229), its authored
light contributions should use this same multiplier, applied once independently
of ambient, emissive contributions, and runtime spotlights (tracked in #1547).

Developer parameters use category submenus. **Back** moves up one category;
**Resume** on the pause Developer page returns directly to gameplay. Opening
Pause still starts at the pause root, but choosing Developer restores the last
category and its scroll position. The main-menu Developer screen shares this
navigation for the lifetime of the application, including across mission changes.

**Visualizations** offers **All on / All off** for all overlays or just a
subcategory. Individual switches stay editable; counts show how many are on.
Hands & zones includes gloves, support grips, clip insertion, ammo pouch,
holsters, and backpack zones. Combat includes creature hitboxes
(`show_hitboxes`), held melee contact volumes, and damage numbers. Bulk controls
only change visualization flags, never fit settings or tuning values.

**Visualizations → Show position** (`show_position`) displays the player's
world X/Y/Z coordinates, updated live in flat and VR. It defaults off and
participates in All on / All off. VR places the shared readout on an upright
panel with lazy recentering; it is hidden while the cyber interface, pause menu,
or death camera owns the view. Values last until app restart.

**Visualizations → Physics wireframe** (`debug_physics`) toggles collider,
contact, and joint debug drawing without restarting the mission. It defaults
off and participates in All on / All off. Desktop and debug runtime
`--debug-physics` starts it on; menu and HTTP changes can still turn it off
immediately. Like other live parameters, it lasts until app restart.

**Locked** contains settled tuning, hidden from the ordinary categories but
editable when opened. HTTP access is unchanged. Initially this includes global
glove forward offset and melee drive limits, swing threshold, and model scale.
Declarations use `Category::float(...)` / `Category::bool(...)`, or
`float_locked(...)` / `bool_locked(...)`; locking only changes menu placement.
`GET /v1/dev-params` also reports each parameter's category label and locked flag.

#### Glove fit check (`debug_gloves`)

Open `debug_gloves` from the Developer scene list. On Quest it requests room
passthrough behind the controller-driven gloves. Desktop/debug use a black
background and synthetic hand input (`cargo dbgr --mission debug_gloves --vr`).
The old pose gallery is replaced by the production glove mesh and analog curls;
`debug_hand_poses` remains available for authored-pose inspection.

Hold Menu to open Pause, then Developer, to adjust these live parameters:

| Label | HTTP key | Meaning |
| --- | --- | --- |
| Glove forward cm | `glove_forward_cm` | Default `−15`; applies globally to VR menus and gameplay. Negative pulls back, positive moves along hand-local -Z. Range ±20 cm, steps of 0.5 cm. |
| Glove side cm | `glove_side_cm` | Default `0`; mirrored controller-local X (positive: right for right hand, left for left). ±10 cm in 0.5 cm steps. |
| Glove up cm | `glove_up_cm` | Default `0`; controller-local +Y, rotating with your hand. ±10 cm in 0.5 cm steps. |
| Glove size | `glove_fit_size` | Default `1`; scales the mesh about its hand origin, from 0.5–1.5. |
| Fit gloves | `glove_fit_visible` | Hide/show the gloves to compare your real hand silhouette. |
| Hand pose | `glove_fit_grip_pose` | Quest only: choose **Aim** (default, current gameplay reference) or **Grip** (experimental holding reference). HTTP stores Aim as `0`, Grip as `1`. |
| Fit passthrough | `glove_fit_passthrough` | Quest only: toggle room/black background. Default on. |

Forward applies to the shared VR hand frame everywhere: gloves, wrist UI, held
items and interaction origins move together. Normal flatscreen gameplay is
unchanged; the flat fit scene uses the same calibration as VR for comparison.
The other six controls apply only to this scene, including its pause-menu
gloves. All controls remain set across scene changes and reset to their defaults
on app restart. Keep controllers in hand: passthrough shows your real
hands, but this experiment does **not** implement optical hand tracking. First
compare aim/grip with offset `0` and size `1`; then adjust one parameter at a
time while holding still and looking at the wrist, palm and fingertips from
several angles. Record pose mode, offset and size together. Grip and aim differ
in rotation as well as position, so translation alone may not align every pose.

Passthrough is optional: unsupported devices or creation failures retain a
black background and emit `SHOCK2QUEST_PASSTHROUGH` diagnostics. Toggle it off
and on to retry creation. Leaving the scene releases its passthrough objects.
Local screenshots verify geometry and tuning, **not** real-hand registration;
physical fit requires a wearer, and compositor/session recovery requires device checks.

Quest 3 device checks confirmed stereo room passthrough and a successful
suspend/return cycle. Repeated Home/reopen cycles can also stall at XR `IDLE`
with loading dots; the same failure reproduces in `debug_minimal` without
passthrough running. A fresh app launch recovers. Physical glove alignment
was checked by a wearer: aim reference with forward **−15 cm** aligned well
with palms facing each other, but gloves sat slightly below real hands with
palms down. At the wearer’s request, −15 cm is now the global forward default;
further orientation-dependent refinement remains open. Use the side
and up controls to test the remaining error, keeping forward and size fixed;
if the wrist aligns but the fingertips diverge, investigate rotation or size.
Menu gloves use the same calibrated hand input as gameplay. Side/up/size
previews only change the fit-scene mesh; beam, hit dot and click targeting
continue to share one pointer pass.

`dark::SCALE_FACTOR` stays constant. It scales loaded geometry, motion and
physics data, and feeds `METERS_PER_WORLD_UNIT`; changing it live would mix old
and new units. Geometry divided by this factor and meters-per-unit multiplied
by it cancel: it is an internal unit convention, not a physical-size slider.
Glove size is deliberately an independent fit experiment and
does not change the world, stereo separation, tracking conversion or saved grips.

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

**Flat scale** is the flatscreen equivalent, and the only recoil row that is
flat-*specific*: the same spring drives the first-person viewmodel, which
kicks back along its own barrel and pitches its muzzle up before settling. It
ranges from `0.25` to `5` in `0.25` steps (default `1`) and rescales the kick
together with its travel caps, so the knob keeps biting at the top of its
range. Flat holds the gun in both hands, so the one-hand penalty never applies
there; **Back/Pitch/Yaw scale**, Strength and Agility still do, underneath.
The flat kick is presentation only - the camera and the crosshair the shot
actually follows never move - so it cannot change your accuracy. Note that at
AGI `6` the authored angular kick is zero (as in the original), leaving only
kickback: drop **Gun AGI ovrd** to `1` to see muzzle rise at all.

**Flat aim follow** (`flat_recoil_aim`, 0-1, step 0.1, default 1) decides how
much of that kick the *shot* rides. VR shots leave along the physically
displaced muzzle, which is why firing faster than the spring recovers walks
your aim up; flat reproduces that by bending the crosshair fire ray by the
viewmodel's own kick. `0` restores purely cosmetic recoil (the shot always
leaves along the crosshair). A settled gun fires exactly on the crosshair at
every setting, so the reticle only ever lies while the gun is visibly
displaced - and the camera never moves. This is a **balance-affecting** knob,
unlike **Flat scale**: it stacks on top of the existing weapon-skill spread.

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

#### Throw tuning

**Developer → Weapons → Throwing** contains the live, unlocked throw controls.
They appear in both the main-menu and pause Developer pages introduced in #1541,
and are also available through `game.devParams.set(key, value)` or
`POST /v1/dev-params`. Values reset on app restart; Reset restores each default.
Launch settings apply on the next release, smoothing on the next motion sample,
and damage settings at impact. Damage window is selected when releasing.

| Key | Default | Controls |
| --- | --- | --- |
| `throw_speed_scale` | 1 | Overall hand-speed gain, before the speed cap |
| `throw_spin_scale` | 1 | Overall angular-speed gain, before the spin cap |
| `throw_max_speed` | 12 | Maximum hand-derived speed in world units/s; player motion is added afterward |
| `throw_max_spin` | 25 | Maximum spin in radians/s |
| `throw_smoothing_ms` | 50 | Recent motion averaging window; 0 uses the latest sample |
| `throw_strength_override` | 0 | 0 follows the character sheet; 1–6 tests Strength without changing saved stats |
| `throw_strength_bonus` | 0.25 | Extra speed at Strength 6, interpolated from no bonus at Strength 1 |
| `throw_weight_exponent` | 0.25 | Slowdown for objects heavier than the reference mug; 0 removes it |
| `throw_strength_weight_relief` | 0.5 | Fraction of heavy-object slowdown removed at Strength 6 |
| `throw_min_speed` | 1.5 | Minimum hand speed in world units/s to arm impact damage |
| `throw_impact_min_speed` | 2 | Minimum closing speed for damage |
| `throw_damage_speed` | 6 | Reference-mug closing speed that reaches its damage cap |
| `throw_organic_cap` | 2 | Whole HP per flesh-target hit, adjustable from 0 to 2 |
| `throw_inorganic_cap` | 1 | Whole HP per other-material hit, adjustable from 0 to 1 |
| `throw_damage_window` | 5 | Seconds after release during which the first contact can damage |

Start with speed/spin scale and smoothing to tune release feel, then compare
Strength 1 and 6 with the same object. The mug uses authored mass 30 as the
reference weight; these are Dark units, not kilograms. First contact spends a
throw even against scenery. Tracking validity, teleport rejection, and the
one-impact rule stay fixed.

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
- Before running for the first time, copy your Remaster KPF archives, mods, and
  cutscenes to the headset. Follow [Copy your game files](INSTALL.md#copy-your-game-files)
  for macOS/Linux and Windows commands.
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

### AI hearing and distractions

AI hearing receives player gunshots, player footsteps/landings, and qualifying
player-caused material impacts (held weapons, released props, and player-fired
projectiles). Creature footsteps, voices, and enemy projectile impacts play
normally without generating investigation cues. Audible playback and AI hearing
are separate; music, narration, ambient audio, and other schema sounds do not
alert enemies merely because they play.

| Noise | Base range (world units), before listener acuity and cover |
| --- | --- |
| Gunshot | 20 |
| Material impact | 8 |
| Walking | 6 at Agility 1, falling linearly to 3 at Agility 6 |
| Crouching | Half the walking range |
| Landing | 1.5 times the walking/crouching range |

These source ranges and the Agility/cover curves are gameplay tuning, not a
claim of exact original-engine parity. `P$AI_Hearin` uses the original default
range multipliers: 0 (deaf), 0.25, 0.65, 1, 1.5, 3 for ratings 0–5. An absent
property means normal hearing (rating 3). Three rays toward the listener sample
solid cover, sharing the explosion ray filtering. Full cover reduces range to
25%; partial cover interpolates toward full range. This approximates muffling,
not sound paths around corners or a room/portal acoustic simulation.

A heard noise supplies the landing/firing/footstep position to the existing AI
investigation behavior. Each fresh heard cue renews the loss-of-contact timer,
including when the AI is already alerted. On arrival, the monster stops and scans
rather than attacking the empty location. Seeing the player again resumes pursuit/combat.
The player can throw a cup away from their hiding place to draw a monster there.
Released props use pre-solve relative contact speed and the existing impact
sound gate (0.1 world units/s, 0.15 s cooldown per contact partner), independently
of throw damage. Contacts below that threshold and `NO_COLLISION_SOUND` props are
silent. Impact noises notify AI only after a sound sample resolves and plays. A rebound may clatter again after the cooldown. Flat inventory tosses
also use this sound path while retaining their existing speed/damage behavior.

During pursuit/search, mobile monsters can also pick up the player's recent
scent after reaching their current seen/heard destination. A mission-local buffer
holds at most 200 grounded positions, sampled every 0.1 simulation seconds with
nearby samples merged. Scent lasts 20 seconds; pickup range fades from 2.5 to
0.75 world units. A solid-cover ray must be clear. Idle monsters do not acquire
scent, and scent does not interrupt travel toward a thrown distraction.

The first pickup chooses the freshest nearby point. Later pickups follow newer
points in order, each discovered locally after reaching the previous goal. There
is no access to remote trail points or the unseen player's live position. Sight
and audible cues keep their existing priority; a trail gap or expiry ends scent
tracking and leaves the normal search/decay behavior. Trail age and each monster's
tracking cursor/destination survive save/load. These are initial tuning constants;
Agility/hearing affect sound detection, not scent lifetime or range.

The acoustic regression tests cover hearing ratings, cover, Agility, and crouch;
SDK scenarios exercise real footsteps, gunfire, and thrown-cup audio/investigation.
Footstep pacing retains its existing limits: tracked room-scale head movement
alone does not move the pawn or generate steps.
