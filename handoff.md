# Handoff — flatscreen/VR frontend menus

Living doc for the menu work. Updated as the investigation proceeds.

## Shipped / in review

| PR | State | What |
| --- | --- | --- |
| [#927](https://github.com/tommy-xr/shock2quest/pull/927) | **merged** | Main menu driven by `MAIN.STR` + `MAINR.BIN`; all six entries, four dimmed |
| [#934](https://github.com/tommy-xr/shock2quest/pull/934) | **merged** | `cargo dbgr` boots the main menu with no `--mission` |
| [#930](https://github.com/tommy-xr/shock2quest/pull/930) | **merged** | Load-game screen, `save_load::all_saves`, `engine::ellipsize` |
| [#953](https://github.com/tommy-xr/shock2quest/pull/953) | open, mergeable, CI green | VR controller pointer + world-space menu panel + VR menu default |
| [#961](https://github.com/tommy-xr/shock2quest/pull/961) | open, CI green | Three UI emit paths -> one layout pass; parity test |

Both open PRs are rebased onto current `main` (#930 was squash-merged, so the
original load-game commits had to be dropped by replaying only the VR commits
with `git rebase --onto origin/main <last-load-game-commit>`).

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

### Still open (as of the #961 stack)

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

## Guiding principle

**UI must render consistently between flatscreen and VR** (repo owner, explicit).
Text rendering or sitting in a different place between the two presentations is
unacceptable — it is a bug of the same severity as wrong content, not a cosmetic
nit. Divergence should be **structurally impossible**, not merely caught by a
test. Now encoded in `AGENTS.md` section 3.

## Renderer consolidation (in progress)

### Why the two presentations disagreed

PR #840 ("unify 2D canvas descriptions") unified the *data model* — one
`UiElement` enum — but left **three independent emit paths**, each re-deriving
placement from the same description:

| Path | Where | Notes |
| --- | --- | --- |
| screen-space | `UiCanvas::render_screen_space_with_pointer` (~159 lines) | aligns, measures, ellipsizes |
| world-space | `UiCanvas::render_world_space` (~143 lines) | learned alignment only in `69e0553`; **still does not ellipsize** |
| GUI / MFD | `GuiComponentRenderInfo::render` (`gui_component.rs`) | keypad, container, replicator, elevator, inventory |

The world path was lifted from the older GUI world-space code, which only drew
fixed-size widgets where alignment never mattered — so it inherited that code's
quirks (Y-rotation instead of Z, negated y, corner anchor) and never grew
alignment. Nothing forces the paths to agree, so each new feature lands in one
and silently misses the others. Live proof: `fit_to_rect`/`ellipsize` is
screen-only, so a VR load screen would spill long save names exactly as the flat
one did before `engine::ellipsize` existed.

### Decision: consolidate first, render-to-texture second

**Chosen:** one canvas-space `layout()` pass producing `PlacedElement`s
(alignment and ellipsize already resolved), consumed by thin affine mappers per
presentation, plus a parity test asserting the mappers agree. Delegated to a
subagent on `refactor/ui-single-layout`, branched from `feat/vr-menu-pointer`
so it starts from verified-working VR with tests that pin the behaviour —
rather than from `main`, where VR is broken and the anchor asymmetry is
undiscoverable without rendering.

### Outcome (landed on `refactor/ui-single-layout`)

`UiCanvas::layout` is now the single placement authority: it resolves
alignment, measures text, applies `ellipsize`, and sizes object icons, yielding
`PlacedElement { rect, alpha, content }` in canvas pixels (for text the rect is
the glyph box, so its height *is* the font size). Screen space and the
world-space panel are mappers over that list with no per-element-kind placement
left, and the GUI/MFD path was folded in: `GuiComponentRenderInfo::render` is
gone, and both the flat overlay and the VR world quad build their elements
through one `to_ui_element` conversion.

The anchor asymmetry that section 3 of `AGENTS.md` warned about was *removed*
rather than confined: `SceneObject::world_space_text` now normalizes its glyphs
into the centered unit square (same shape as `quad::create`), so world-space
text is placed by the very `world_element_transform` call an image uses.

Two real divergences this fixed, both visible on the VR main menu:

- world-space text was drawn at a **fixed** 0.045-of-panel-height regardless of
  the layout's font size (~40% too large for the menu font, spilling out of the
  button pills and over the numeral art);
- it carried a half-line anchor fudge, so a `VAlign` case landed the label
  higher than the flat presentation put it.

VR MFD panel text had the same fixed-size problem relative to its own panel
pixels; it now matches the flat MFD.

Still open: the load-game screen has **no** world-space presentation at all —
in `--vr` it falls back to the screen-space overlay (`LoadGameScene::render`
returns nothing; `render_per_eye` draws for both modes). So the one canvas that
exercises `fit_to_rect` is never actually shown on a panel. Ellipsizing now
happens in layout, so it *will* be correct when that screen grows a VR panel.

**Deferred: render-to-texture for world space.** Rendering the canvas to an
offscreen target and mapping it onto the panel quad would make parity true *by
construction* (one renderer, not two that agree). It is the better end state,
but:

- `engine` has **no render-target abstraction at all** — the only FBO code is
  `oculus_runtime`'s XR swapchain — so it needs one for desktop GL *and* Android
  GLES.
- Text sharpness becomes resolution-bound (fixed-size texture vs direct
  geometry) and Quest memory/perf matters. Neither is measurable without a
  headset.

Consolidation makes world-space a thin mapper, so swapping that mapper for an
RTT quad afterwards is a small contained change that can be benchmarked on
device. Doing RTT first would bet the refactor on unverifiable engine work.

## What is left

- **No hardware verification.** Everything is the debug runtime's `--vr` path; the
  Quest `DEFAULT_MISSION = main_menu` change is untested on device.
- **The load screen has no VR presentation** - in `--vr` it falls back to the
  screen-space overlay, so the one canvas using `fit_to_rect` never appears on a
  panel. Small job now that `WorldPanel` + the layout pass exist.
- **Render-to-texture for world space** remains the better end state (parity by
  construction rather than by test). `engine` still has no render-target
  abstraction; consolidation has made the world mapper thin enough that swapping
  it for an RTT quad is now a contained change.
- `ScaleMode::Stretch` on a mismatched aspect is the one residual divergence; no
  shipped caller uses it, and it is documented on the enum.
- A full e2e suite run before landing (`missions.e2e` 23/23 and a UI subset have
  been run; the full suite has not, on the final tree).

## Process notes worth keeping

- **Never script `git rebase --skip` across conflicts.** Doing so silently
  discarded four substantive commits here; only a pre-made backup branch saved
  them. Inspect every conflict; skip only after positively verifying the content
  is already upstream.
- **Launch subagents with worktree isolation** when they will touch git state.
  A non-isolated agent shares the checkout, so its branch switches land your
  commits on its branch and block your own git operations.
