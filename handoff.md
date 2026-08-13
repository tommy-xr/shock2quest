# Handoff — flatscreen/VR frontend menus

Living doc for the menu work. Updated as the investigation proceeds.

## Shipped / in review

| PR | State | What |
| --- | --- | --- |
| [#927](https://github.com/tommy-xr/shock2quest/pull/927) | **merged** | Main menu driven by `MAIN.STR` + `MAINR.BIN`; all six entries, four dimmed |
| [#930](https://github.com/tommy-xr/shock2quest/pull/930) | open, CI green | Load-game screen (`GAMELOD.*`), `save_load::all_saves`, `engine::ellipsize` |
| [#934](https://github.com/tommy-xr/shock2quest/pull/934) | merged into #930's branch | `cargo dbgr` boots the main menu with no `--mission` |
| `feat/vr-menu-pointer` | local, **VR working end to end** | VR controller pointer + world-space menu panel + VR menu default |

Open issues: [#928](https://github.com/tommy-xr/shock2quest/issues/928) (load list has no scrolling past 14 rows), [#929](https://github.com/tommy-xr/shock2quest/issues/929) (failed load is silent).

## Key data facts

- Frontend screens are fully data-driven: `<SCREEN>.PCX` + `<SCREEN>R.BIN` (LTRB `int16` rects) + `<SCREEN>.STR`.
- **`.STR` keys are listed in REVERSE screen order.** `MAIN.STR` is quit/intro/credits/options/load_game/new_game = bottom-to-top. Confirmed by the backdrop's own numbered slots 1-6.
- SCP ships redrawn backdrops **with retuned `*R.BIN`** (e.g. `SIMR.BIN` row 0 `400,20 179x76` -> `417,22 158x69`), so hardcoded rects are wrong on modded/25AE installs.
- 25AE keeps the legacy frontend intact (`sshock2.kpf`); its remaster layer has **zero** `intrface/` files. A KEX-style menu would be from scratch: no layout data, `@2x` BC7 DDS art, TTF fonts (we have no TTF support).

## RESOLVED: the world-space menu panel now renders

**Root cause: there is no canonical "forward" axis to hardcode.** The panel was
pinned to a fixed `-Z` offset from the scene origin, but the runtimes' yaw-0
camera looks along **+X**:

```rust
// runtimes/debug_runtime/src/main.rs
fn head_rotation_from_yaw_pitch(yaw_deg, pitch_deg) {
    let forward = point3(yaw.cos()*pitch.cos(), pitch.sin(), yaw.sin()*pitch.cos());
```

At yaw 0 that is `(1,0,0)`. (The doc comment on the `head.look` channel claiming
"forward = -Z" is wrong.) The panel sat ~90 degrees off to the side, permanently
outside the frustum — which explains every symptom: only an enormous panel's edge
crept into view, and equal width/distance ratios produced byte-identical slivers.

**Fix** (`5c8cba1`): place the panel at `head + (head_rotation * -Z) * distance`
and orient it by looking from the panel back to the head — i.e. exactly what
`DebugMapScene` does. `head_rotation` is now tracked on **every** update, not
only on the VR branch (that omission silently left it at identity and cost an
extra round of debugging).

**Second fix** (`69e0553`): world-space text ignored its rect and alignment, so
labels floated up-left of their buttons. It now aligns like the screen-space
path — with the caveat that `SceneObject::world_space_text` anchors on the
text's vertical **centre** while the screen-space path anchors on its **top**,
so each `VAlign` case carries half a line.

### How it was found

Bisection by transplant: swap the menu's canvas into `debug_map` (renders) and
`debug_map`'s canvas into the menu (blank). That isolated *transform* from
*content* in one step, after a long stretch of fruitless parameter tweaking.
Worth reaching for much earlier next time.

### Third fix: the VR view was inside a red box

`Game::render_per_eye` appended a red `color_material` cube **in VR only**:

```rust
from_scale(0.25) * from_translation(vec3(0.0, 4.0, 0.0))   // -> centred at (0, 1.0, 0)
```

The VR camera sits at eye height `(0, 1.04, 0)` — *inside* a cube spanning
+-0.125 — so every VR frame rendered the inside of a red box and hid the scene.
That is why `debug_map` was blank in VR too, which made it look like a menu
problem. An earlier comment ("keep it out of the clean flatscreen view") had
gated it to VR rather than deleting it, hiding the damage in the mode nobody
screenshotted. Removed in `4ce6934`.

### VR now works end to end

- Panel renders in VR (7 canvas objects, 14618 distinct colours).
- Controller ray **hover** highlights the entry under it ("New Game" bright,
  "Load Game" dim).
- Trigger **activates**: aiming at "New Game" and pulling took the scene from
  0 entities to 747.
- `--vr` with **no `--mission`** boots the menu, so the VR special case in
  `desktop_runtime` is gone and `oculus_runtime`'s `DEFAULT_MISSION` is
  `main_menu` (`8a0a1c3`).

### Still open

- The label drifts a few px right of centre — world text likely renders at a
  slightly different scale than `measure_text_width` reports.
- The **load screen** has no VR presentation yet, so "Load Game" in VR opens a
  screen that is still flat-only.
- Not verified on real hardware (no headset here); everything above is the
  debug runtime's `--vr` path.

## Method traps (both cost real time)

- **PNG byte size is not a content signal.** A solid image compresses identically
  at any colour, so "same bytes" does NOT mean "same picture". Count colours:
  `len(Image.open(p).convert('RGB').getcolors(maxcolors=300000))`.
- **Flat clears black, VR clears red**, and `MAIN.PCX` is mostly black art — so
  "not rendering" and "rendering dark art" look identical.
- **zsh does not word-split unquoted vars**, so `for cfg in "2 1.5 2"; do set -- $cfg` silently runs one config. Use `bash <<'EOF'` for sweeps.
- The debug runtime **ignores `should_quit`**, so menu Quit paths cannot be verified there.
- A silent `str.replace` in a patch script can no-op and send you chasing a
  phantom; assert the pattern matched.

## Repro

```bash
# Fast iteration: panel is env-tunable on the branch's experiment build
PANEL_W=8.0 PANEL_H=6.0 PANEL_D=2.0 DARK_ASSET_PATH=~/ss2-25th \
  ./target/debug/debug_runtime --mission main_menu --port 8140
curl -s -X POST localhost:8140/v1/step -d '{"frames":6}'
curl -s -X POST localhost:8140/v1/screenshot -d '{"filename":"/tmp/p.png"}'
```

## Design note (not yet implemented)

VR has a **head position**, not just a rotation. `InputContext` currently exposes
`head.rotation` only, so the panel is placed relative to the scene origin plus the
runtime's fixed eye-height offset. A real VR menu should anchor to the tracked head
pose (place once in front of the player on entry, or soft-follow), which needs head
position plumbed into `InputContext`.
