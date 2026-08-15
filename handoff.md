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

---

# OPEN: every world-space UI panel is 180 degrees rotated on a real headset

Added 2026-08-14. The section above ("VR now works end to end") was verified with
`cargo dbgr --vr` — **never on a Quest**. On device it does not work, and neither
does anything else drawn on a world-space panel.

## Symptom

On a Quest 3, world-space UI renders rotated 180 degrees in-plane: text mirrored
and upside down, and vertical order reversed (the main menu lists Quit at the top
and New Game at the bottom). The **3D world renders correctly** — only panels are
wrong.

It is **not** scene-specific. Both are inverted identically:

- the VR main menu, via `ui::frontend_panel`
- `debug_map`, which places a panel **entity** and never calls `frontend_panel`

So the cause is below both. The same builds render both panels **upright** under
`debug_runtime --vr`.

## Why nobody caught it

`debug_runtime --vr` renders these panels correctly, so every screenshot, both
code reviews, and the section above all passed. **The harness does not model the
device for world-space UI.** Until that is fixed, a desktop VR capture is not
evidence about VR.

## Ruled out ON HARDWARE (do not re-try these)

Each cost a build/install/capture cycle. All were reverted.

| # | Attempt | Result |
| --- | --- | --- |
| 1 | Flip `frontend_panel`'s basis (negate `right` + `true_up` — a roll about Z) | Device unchanged; **desktop becomes mirrored**. Moves the bug between runtimes. |
| 2 | Replace `world_element_transform`'s `from_angle_z(180)` with `from_nonuniform_scale(1,-1,1)` | **Fails the flat/VR parity tests.** The canvas->panel mapping genuinely needs both axes flipped *unless* `canvas_rect_to_panel` changes with it (see #3). |
| 3 | The full atomic 5-site version (below) | **Desktop correct for both panels** (`debug_map` upright and readable for the first time); **device unchanged**. |
| 4 | Platform-conditional yaw: `cfg!(target_os = "android")` picking `from_cols(-right, true_up, look_dir)` | Device still mirrored, **and the menu labels vanish entirely**. Strictly worse. |

**#3 is the important negative result.** With a fully honest basis and no
compensation anywhere, the desktop is right and the headset is still wrong. That
rules out the UI math — basis, canvas mapping, and layer depth — as the cause.
It also refutes the natural theory that "desktop is only correct by
compensation".

### The 5 coupled sites (if you touch one, you touch all of them)

Found by a Codex pass; verified by the parity tests catching an incomplete version.

1. `shock2vr/src/ui/mod.rs` `world_element_transform` — `from_angle_z(Deg(180.0))`, which flips x as well as y
2. `shock2vr/src/ui/mod.rs` `frontend_panel` — basis with `right = look_dir.cross(up)` (negated) and third column `-look_dir`
3. `shock2vr/src/scenes/debug_map.rs` (~line 229) — a duplicate of that same basis
4. `shock2vr/src/ui/mod.rs` `canvas_rect_to_panel` — `vec2(0.5 - p.x, 0.5 - p.y)`, whose doc says it exists to undo the double negation. **This is what the parity tests pin**; with a y-only flip it becomes `vec2(p.x + 0.5, 0.5 - p.y)`.
5. `shock2vr/src/ui/mod.rs` `render_world_space` — layer z-step direction, which follows the panel's facing convention

## Verified true (don't re-derive)

- **Hit detection follows the panel automatically.** `ray_to_canvas` intersects
  using `panel.normal()` and un-rotates by `panel.rotation`, both taken from the
  panel's own transform. A basis change carries the pointer with it; input cannot
  desync from what is drawn.
- **`PresentationMode::Vr` cannot distinguish the two runtimes** — desktop `--vr`
  and the headset are both `Vr`. Any conditional has to be by platform.
- **`input_context.head.rotation` used to be the RIGHT CONTROLLER's aim pose**
  (`right_aim_location`), and is the *zero quaternion* when controllers are
  untracked — `rotate_vector` by it silently returns the vector unrotated, so the
  panel pinned itself to world `-Z`. Fixed in #994; the panel now tracks the head.
  This was a real bug but is **not** the rotation cause.

## Leading untested hypothesis

`ui/mod.rs` `frontend_panel` anchors at `let head = vec3(0.0, FRONTEND_PANEL_EYE_HEIGHT, 0.0)`
— the **world origin**, not the player's tracked position. Desktop's camera sits
at the origin so it works there; on a headset you are offset roomscale, and a
double-sided quad viewed from behind reads exactly as mirrored.

Cheap probe: anchor the panel to the actual camera/eye position and re-capture.
Note `debug_map` places its panel relative to the player entity, so if this is
the cause it must explain that scene too.

## BREAKTHROUGH: this reproduces on the DESKTOP — no headset needed

The transplant probe recommended below was run, and it changes the whole shape
of this problem.

**Experiment.** In `NoAssetsScene::render`'s VR branch, emit a raw
`SceneObject::world_space_text` placed with the *same* `panel.center` and
`panel.rotation` as the canvas, i.e. bypassing `world_element_transform`.
Placement is held constant; only the element path varies.

**Result:**

| | canvas text | raw probe |
| --- | --- | --- |
| `debug_runtime --vr` (desktop) | upright | **180 degrees rotated** |
| Quest 3 | 180 degrees rotated | **180 degrees rotated** |

**The probe is inverted on BOTH platforms.** So a raw world-space quad placed
with the panel's own transform renders upside down *on the desktop too* — and
`world_element_transform`'s `from_angle_z(180)` is the compensation that hides
it there.

### Why this matters

The panel basis being 180 degrees off is **not** device-specific, and it is
**desktop-reproducible**. That converts a headset-only bug, costing a
build/install/capture cycle per idea, into something iterable in seconds with
`cargo dbgr --vr`. Do that work on the desktop first.

The success criterion to iterate against is now concrete and does not need a
Quest: **make the raw probe render upright while the canvas stays upright.**
Today exactly one of the two can be right at a time, which is the real defect —
two paths to the same panel disagree by 180 degrees, and the canvas path is only
correct because a hardcoded rotation cancels the error.

### The one thing still unexplained

If the panel basis is 180 off on both platforms, and the element path adds 180
on both, the canvas should be correct on both. It is not: the canvas is upright
on desktop and inverted on device, while the probe is identical on both. Attempt
#3 (the honest 5-site version) also fixed the desktop canvas without changing
the device. So there is a second, device-only factor on top of the shared basis
error. Fix the shared, desktop-reproducible error first — it is likely to make
the remaining device delta much easier to see.

## Recommended technique

This document's own advice from the last round applies directly: **bisection by
transplant, early.** Four parameter guesses cost four device cycles and produced
one useful negative. Instead, put a deliberately asymmetric test quad (e.g. an
"F"-shaped texture) on device through the world-space path and see whether it is
mirrored, rotated, or fine — that separates *transform* from *content* in one
cycle, and would distinguish "panel viewed from behind" from "content built
upside down".

## Device workflow notes

- The headset sleeps within seconds when not worn, and an unfocused app renders
  nothing — compositor captures come back **black or zero bytes**. Fix:
  `adb shell am broadcast -a com.oculus.vrpowermanager.prox_close`, then
  `adb shell input keyevent KEYCODE_WAKEUP`. Without this, none of the above is
  observable.
- Capture: `node .claude/skills/vr-device-loop/scripts/quest-device.mjs --serial <S> capture out.png`.
- Scene selection is `/sdcard/shock2quest/vr-mission.txt` (e.g. `main_menu`,
  `debug_map`). **Restore it to `medsci1.mis` when done.**
- `println!` reaches logcat; `tracing::info!` does **not** — neither
  `desktop_runtime` nor `oculus_runtime` installs a subscriber. Instrumenting
  `frontend_panel` with a rate-limited `println!` of the head quaternion and
  basis vectors is what found the zero-quaternion bug, and is the fastest way in.

## PR stack

- [#993](https://github.com/tommy-xr/shock2quest/pull/993) `feat/no-assets-screen` -> `main` — missing-assets screen (inherits this bug in VR)
- [#994](https://github.com/tommy-xr/shock2quest/pull/994) `fix/vr-frontend-panel-orientation` -> **#993** — the head-pose fix; carries comments at the traps above

## RESOLVED (desktop): the shared basis error is fixed — the basis is now honest

Landed on this branch (2026-08-15). The "honest" change is now in, atomically,
at **six** sites — the handoff's five plus one it missed:

1. `world_element_transform`: the `from_angle_z(180)` compensation is gone. The
   one flip that remains is the canvas-y flip (canvas y grows down, panel y
   grows up), done as `from_angle_x(180)` — a proper rotation, so triangle
   winding is preserved (a `scale(1,-1,1)` mirror would flip winding).
2. `frontend_panel`: basis is `right = up x look_dir` (the viewer's right),
   `true_up = look_dir x right`, third column **+look_dir** — local +Z points AT
   the viewer. `WorldPanel::normal` is now genuinely the outward face.
3. `debug_map.rs`: same basis change (its duplicate copy).
4. `canvas_rect_to_panel`: corner map is `(p.x + 0.5, 0.5 - p.y)`.
5. `render_world_space`: layers step **+z** (toward the viewer).
6. **`gui_manager.rs` (the missed site)**: the MFD proxy entity's outward face
   is its local -Z, so its root transform now carries an explicit
   `from_angle_y(180)` at that one boundary, commented. Without this the honest
   element path would have turned every in-game MFD panel around.

Also fixed by this, silently: `ray_to_canvas` hit-testing was **horizontally
mirrored** against the old basis (a hit at canvas x mapped to 640-x). Nobody
noticed because the menu buttons are full-width rows and the unit tests invert
`ray_to_canvas` to build their hand poses, so the error cancelled. It is gone
now (`u = cx` under the honest basis), and a click aimed at New Game's true
rect center was verified to fire New Game (0 -> 747 entities).

### What the probe actually shows now (calibration for the device pass)

With the honest basis, the raw `world_space_text` probe renders **y-flipped
only** — letter order correct left-to-right, each glyph vertically mirrored —
NOT 180-rotated. That is not a basis error: `unit_text_vertices` (and every
raster texture) is authored y-down, and the single canvas-y flip that corrects
it lives in `world_element_transform`. In other words the earlier "probe is 180
rotated on both platforms" was y-down content x the old basis's hidden
turn-around; the turn-around is gone and the y-down convention remains, applied
once, on purpose, for all content kinds equally.

**Device implication**: re-run the probe on the Quest against this branch.
Expected if the device shares desktop behavior: probe y-flipped, canvas
upright. If the canvas is still 180-rotated on device, the delta is now
guaranteed to be outside the UI math entirely (basis, element path, and canvas
mapping are all honest and desktop-verified). `SHOCK2_PANEL_LOG=1` makes
`frontend_panel` print the head quaternion + derived basis via `println!`
(reaches logcat), rate-limited — compare desktop vs device readings directly.

### Desktop verification evidence (all `cargo dbgr`, DARK_ASSET_PATH=~/ss2-data-unpacked)

- VR menu: canvas upright, New Game top / Quit bottom, labels in their pills.
- Raw probe: y-flip only (letter order preserved) — basis honest.
- `debug_map --vr`: panel upright and readable (ELEVATOR / CRYO / key legend).
- Hover: hand ray aimed at New Game's rect center brightens exactly New Game.
- Click: trigger pull on New Game transitioned the scene, 0 -> 747 entities.
- Flat menu: pixel-identical before vs after (ImageChops diff bbox = None).
- `cargo test -p shock2vr`: 682 passed, zero expectation changes — the parity
  tests pass unchanged because element path and corner map moved together.
- `RUSTFLAGS="-D warnings" cargo check -p shock2vr -p desktop_runtime -p debug_runtime` clean.

## RESOLVED (device): there was no second device-only factor

Verified on the Quest 3 (2026-08-15, release APK of this branch at `b698fe4`):

- **`main_menu`: upright.** New Game top, Quit bottom, every label readable
  inside its pill. Compositor capture, real tracked head pose.
- **`debug_map`: upright.** ELEVATOR / CRYO / SCIENCE / MEDICAL SCIENCE legend
  all read correctly; no mirror, no rotation.
- The `SHOCK2_PANEL` logcat line shows the same honest basis as desktop
  (right = viewer's right, normal pointing back at the head), fed by a real
  tracked quaternion — e.g. `right=(1,0,0) up=(0,0.97,0.22)
  normal=(0,-0.22,0.97)` for a headset resting pitched up.

So the honest basis fix resolved **both** platforms. The earlier on-hardware
negative result for attempt #3 ("desktop correct, device unchanged") was almost
certainly a stale or incomplete device build — the very trap this document
warns about twice (silent patch no-ops, installs not matching the tree). The
"one thing still unexplained" section above is thereby explained: there was
never a second factor, only bad device evidence.

Note for future Android probing: `SHOCK2_PANEL_LOG` is env-gated and env vars
do not reach an Android app — the device run above used a throwaway local edit
(`cfg!(target_os = "android") ||` in the gate) that was reverted after capture.

## RESOLVED (desktop `--vr` interaction): input routing, not geometry

The desktop `--vr` menu being dead was a third, separate defect. The suspected
ray inversion was a **false lead**, measured rather than argued: desktop's
`camera_rotation` does map local +Z onto `camera_forward`, but the rig defines
its own forward as `-camera_forward`, so the negations cancel — eye direction,
panel placement, and hand aim all agree, and `ray_to_canvas` returns a real hit
for the resting pose. The tell: the hit point was not wrong, it was *constant*.
`MainMenuScene::wants_pointer()` is unconditionally true, and the rig answered a
pointer request by zeroing mouse deltas — the flatscreen cursor bargain —
freezing the only thing that aims the hand rays.

Fix: one routing decision, `mouse_look_target`. E/Q still aim their hand;
no-pointer still turns the head; pointer+Flat still surrenders to the cursor
(unchanged); pointer+VR routes the mouse to the **right hand** — head-aiming
cannot work because the panel is head-anchored, so head and ray move together.
An undriven hand's mouse-button state is cleared each frame so a press released
out of order cannot latch the trigger and swallow the menu's rising edge.

Unverified (needs an interactive window): mouse sensitivity — VR keeps the
cursor in Normal mode, so deltas are per-event and `delta_time`-scaled; finite
screen travel may under-rotate the hand. If it bites, capture the cursor for
raw relative motion while a VR panel is up.

---

# CLOSED OUT (2026-08-15): the VR menu effort is a 4-PR stack, all evidence on the PRs

```
main
 └─ #994  fix(vr): head-anchored panels, honest basis, desktop --vr input
     ├─ #997  feat(ui): VR pointer + world panels for load/game-over  (base: #994)
     │    └─ #998  feat(vr): head-anchored panels with lazy recenter  (base: #997)
     └─ #996  feat(oculus): remote input injection over adb           (base: #994)
```

- **#994** carries this document's fixes: head (not controller) anchoring, the
  honest basis (both platforms; the "second device factor" was a stale APK),
  and the desktop --vr mouse→hand-ray routing. Device stills + desktop GIF are
  embedded in the PR. The menu-audio e2e test was re-aimed for the honest
  mapping (it encoded the old mirrored canvas+x→world+z).
- **#997** gives LoadGameScene and GameOverScene the VR pointer + world panel
  (shared `vr_frontend_pointer`, untracked-hand guard, rising-edge fixes). Full
  VR flow verified: menu → load → row → Done → menu; death → game-over → LOAD.
- **#998** replaces gaze-glued panels with place-on-entry / gravity-aligned /
  world-locked / lazy-recenter (`FrontendPanelAnchor`), with `head.position`
  plumbed through all three runtimes. Device before/after in the PR: panel
  moved from ~600 px above eye-centre (inverted disparity) to centred, face-on.
- **#996** adds the Quest remote input server (`/sdcard/shock2quest/debug-port.txt`
  → loopback HTTP over `adb forward`, shared `shock2vr::input::remote` channel
  vocabulary, requires `android.permission.INTERNET` — Android denies even
  loopback bind without it). Used on-device to hover + click the menu with the
  headset resting on a desk: that capture IS the device interaction proof.
- Follow-ups filed: #999 (`debug_map` panel ~17° above gaze — hardcoded 1.5 wu
  head height, pre-existing). Noted in #998: the anchor is per-scene, so the
  panel re-places on each frontend screen swap; hoist one anchor if that feels
  jumpy on head.
- Agent-workflow trap recorded: worktree-isolated subagents branch from
  origin/main, NOT the session's branch — two agents built on the wrong base
  and needed rebase + re-verification. Verify `git merge-base` before stacking.
