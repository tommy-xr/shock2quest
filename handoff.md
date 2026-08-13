# Handoff — flatscreen/VR frontend menus

Living doc for the menu work. Updated as the investigation proceeds.

## Shipped / in review

| PR | State | What |
| --- | --- | --- |
| [#927](https://github.com/tommy-xr/shock2quest/pull/927) | **merged** | Main menu driven by `MAIN.STR` + `MAINR.BIN`; all six entries, four dimmed |
| [#930](https://github.com/tommy-xr/shock2quest/pull/930) | open, CI green | Load-game screen (`GAMELOD.*`), `save_load::all_saves`, `engine::ellipsize` |
| [#934](https://github.com/tommy-xr/shock2quest/pull/934) | merged into #930's branch | `cargo dbgr` boots the main menu with no `--mission` |
| `feat/vr-menu-pointer` | **local, blocked** | VR controller pointer + VR menu presentation |

Open issues: [#928](https://github.com/tommy-xr/shock2quest/issues/928) (load list has no scrolling past 14 rows), [#929](https://github.com/tommy-xr/shock2quest/issues/929) (failed load is silent).

## Key data facts

- Frontend screens are fully data-driven: `<SCREEN>.PCX` + `<SCREEN>R.BIN` (LTRB `int16` rects) + `<SCREEN>.STR`.
- **`.STR` keys are listed in REVERSE screen order.** `MAIN.STR` is quit/intro/credits/options/load_game/new_game = bottom-to-top. Confirmed by the backdrop's own numbered slots 1-6.
- SCP ships redrawn backdrops **with retuned `*R.BIN`** (e.g. `SIMR.BIN` row 0 `400,20 179x76` -> `417,22 158x69`), so hardcoded rects are wrong on modded/25AE installs.
- 25AE keeps the legacy frontend intact (`sshock2.kpf`); its remaster layer has **zero** `intrface/` files. A KEX-style menu would be from scratch: no layout data, `@2x` BC7 DDS art, TTF fonts (we have no TTF support).

## Current blocker: the VR menu panel renders only when enormous

`MainMenuScene::render` presents its `UiCanvas` on a `WorldPanel` via
`render_world_space`. The panel is blank at sane sizes.

### Measured (D = 2m, 45-degree FOV, flat mode with the world path forced)

| Panel W | Result |
| --- | --- |
| 2.0 | blank |
| 4.0 | blank |
| 6.0 | blank |
| 8.0 | **renders** (479 colours) |

Equal `W/D` ratios give **byte-identical** images (W=8/D=2 == W=20/D=5), which is
what proves the transform maths is sound.

### Verified correct by instrumentation

```
root = translate(0, 1.04, -2) * scale(2.0, 1.5, 1)
elem0 corners -> (-1.000, 0.290, -2.000) .. (1.000, 1.790, -2.000)
```

Camera is at `(0, 1.04, 0)` (scene returns origin; the runtime adds
`head_offset = player_eye_height / SCALE_FACTOR = 1.04`). Visible half-extents at
z=-2 are +-1.10 x +-0.83. The panel is +-1.00 x +-0.75 — **fully inside the
frustum, centred** — and still black.

### Ruled out

- Facing (tested both orientations; `gl::CULL_FACE` is commented out in `gl_engine`)
- Eye height / vertical placement, and straddling the screen edge
- `force_alpha` and the `pointer` argument (matched `debug_map`'s values exactly)
- VR specifically — blank in flat too, with the world path forced
- Frustum culling — `gl_engine::render` draws every object unconditionally
- Size alone — `debug_map` renders a **smaller** panel (1.28x0.96) via the same call

### Next suspect

`SceneObject::draw_opaque` only draws geometry when
`material.draw_opaque(..)` returns **true**; otherwise the object is expected to
be picked up by the transparent pass. Check what `basic_material`'s
`draw_opaque`/`draw_transparent` return for these objects' alpha, and whether
either pass actually issues a draw call.

## Method traps (both cost real time)

- **PNG byte size is not a content signal.** A solid image compresses identically
  at any colour, so "same bytes" does NOT mean "same picture". Count colours:
  `len(Image.open(p).convert('RGB').getcolors(maxcolors=300000))`.
- **Flat clears black, VR clears red**, and `MAIN.PCX` is mostly black art — so
  "not rendering" and "rendering dark art" look identical.
- **zsh does not word-split unquoted vars**, so `for cfg in "2 1.5 2"; do set -- $cfg` silently runs one config. Use `bash <<'EOF'` for sweeps.
- The debug runtime **ignores `should_quit`**, so menu Quit paths cannot be verified there.

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
