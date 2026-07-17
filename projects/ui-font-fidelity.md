# UI Font Fidelity (flat UI text sizing)

Status: **in progress** — first slice landed (native-size seam + HUD/menu/panel
migration). Remaining screens (research/keypad MFDs, log reader, save/load) are
follow-ups.

## Problem

Font sizing across the whole flat UI reads "way off" from the original System
Shock 2. The original renders all UI text with **Dark bitmap fonts** (`.FON`) at
their **native pixel size** on the 640×480 canvas — bitmap fonts are drawn 1:1,
never scaled. Our flat UI parses and renders the same `.FON` fonts, but every
call site passed an **ad-hoc font size** larger than the font's native height,
so text came out ~1.4–1.8× too big and, for panels, wildly oversized.

The most visible symptom is the flat HUD bio-monitor: the health/PSI numbers
("100"/"80") render so large they nearly overflow the BIO.PCX backdrop.

## What the engine does (the seam)

Text rendering is already centralized and already uses the real fonts:

- **Fonts**: `dark/src/font.rs` parses Dark `.FON` bitmap fonts (mono
  `format=0`, antialias-16 `format=0x0001`, and anti-aliased `format=0xCCCC`)
  into a glyph atlas + per-glyph advance/`base_height` (native pixel height).
  Exposed through the `engine::Font` trait and `dark::importers::FONT_IMPORTER`.
- **Primitive**: `SceneObject::screen_space_text` (`engine/src/scene/scene_object.rs`)
  lays out glyphs at `font_size` px; `multiplier = font_size / base_height`.
- **Canvas**: `shock2vr/src/ui/mod.rs` (`UiCanvas`) describes UI in **640×480
  canvas pixels**; `render_screen_space` maps canvas→screen (`scale`, letterbox)
  and draws each `Text` element via `screen_space_text` with
  `font_size = size * scale.y`. Alignment (`HAlign`/`VAlign`) is resolved here.

So `size` on a canvas `Text` element **is** the glyph pixel height on the
640×480 canvas. The seam is correct; the callers passed the wrong `size`.

### Fonts and their native heights

Decoded from the shipped `.FON` files - `res/fonts/*.FON` plus the interface
fonts in `res/intrface/` (header: `format@0`, `first@0x24`, `last@0x26`,
`width_offset@0x48`, `bitmap_offset@0x4c`, `row_width@0x50`, `num_rows@0x52`).
"Native height" is the full glyph-cell height (`num_rows`), which includes the
font's internal leading - the visible cap/digit ink is shorter (noted where it
matters):

| Font          | format | native height (px) | notes                        |
| ------------- | ------ | ------------------ | ---------------------------- |
| METAFONT.FON  | 0x0001 | **20** (cap ink 10) | the default GUI style font - main menu et al (`res/intrface/`) |
| MAINFONT.FON  | 0 mono | **11** (digit ink 8) | the general UI/HUD font we use |
| MAINAA.FON    | 0xCCCC | **12** (digit ink 8) | the AA shock font - the original's HUD/panel face |
| BOLDAA.FON    | 0xCCCC | 14                 | (unused today)                |
| BLUEAA.FON    | 0xCCCC | 20                 | (unused today)                |
| DIMMED.FON    | 0xCCCC | 12                 | (unused today)                |
| bignum.fon    | 0 mono | 22                 | big numerics (unused today)   |
| numfont.fon   | 0xCCCC | 11 (digit ink 11)  | large numerics (unused today) |
| keyfont/KEYFONTA | -   | 22                 | (unused today)                |

Only **mainfont.fon** (13 call sites) and **metafont.fon** (menu) are referenced
in code today.

Format `0x0001` ("antialias-16", e.g. METAFONT) stores one byte per pixel with
**coverage levels 0..=15**; the parser scales those linearly to 0..255 alpha
(15 -> 255). Treating them as direct byte alpha - what the parser did before
format-1 support - renders the whole font at max alpha 15/255, i.e. nearly
invisible.

## Gap table (element → original vs. ours before the fix)

| Element                    | Font (native px) | Our size (before) | Delta        |
| -------------------------- | ---------------- | ----------------- | ------------ |
| Main-menu buttons          | metafont (20)    | mainaa at 19      | wrong face: ~½ the original's bulk (see below) |
| HUD health/PSI numbers     | mainfont (11)    | 16                | ~1.45× big   |
| HUD ammo count             | mainfont (11)    | 18                | ~1.6× big    |
| HUD ammo-type label        | mainfont (11)    | 12                | ~1.1× big    |
| HUD PSI power name         | mainfont (11)    | 10                | ~0.9× (ok)   |
| Panel text (elevator, MFD) | mainfont (11)    | **= rect height** (e.g. 20) | ~1.8×, and coupled to the box size — a plain bug |

**Menu correction (review fix):** the first draft of this table compared the
menu against the wrong reference face. The original's main menu uses the
default GUI style font - **METAFONT.FON**, a 20px-cell antialias-16 display
face (cap ink 10px, 'N' 16px wide, "NEW GAME" = 120 canvas px). `mainaa`
(12px cell, cap 8px, "NEW GAME" = 63px) at native size is ~half the original's
bulk. The native-size premise was right; the face was wrong. The menu now
renders METAFONT via the same `text_native` seam.

The panel path was the worst offender: `flat_ui_host` drew `GuiComponent::Text`
with `size = rect.h` (the component's *bounding-box* height, not a font size), so
a 20px-tall floor-label box rendered 20px text.

## Root cause

Two independent issues; **(1) dominates**:

1. **Sizing**: UI text was rendered at ad-hoc `size` constants larger than the
   font's native pixel height. Bitmap fonts must render at native height (1:1)
   to match the original. This is the "way off across the whole flat UI"
   complaint.
2. **AA glyph parsing (minor, deferred)**: the `.FON` parser's `0xCCCC` branch
   has `if alpha > 205 { alpha = 0 }`. Those bytes are *palette-resolved
   coverage* values, not straight alpha - in MAINAA only 55 of 10452 bitmap
   bytes exceed 205 (max 209), so the clamp barely shows there, but the proper
   fix is palette/coverage normalization for the `0xCCCC` fonts (distinct from
   the format-`0x0001` 0..15 -> 0..255 scaling, which is exact and already
   implemented). **Left as a follow-up** (see below).

## The fix (this PR)

Route the correction through the seam so it generalizes:

- `UiCanvas::text` now treats `size <= 0.0` as a sentinel meaning **render at the
  font's native `base_height`** (resolved at render time, where the font is
  loaded). New convenience `UiCanvas::text_native(...)` is the fidelity-correct
  default.
- Migrated the three flat-UI text paths to native sizing:
  - `hud/flat_hud.rs` — all HUD text (`text_native`); removed the ad-hoc size
    constants.
  - `scenes/main_menu.rs` — menu labels (`text_native`); removed
    `MENU_FONT_SIZE`. Review fixes then switched the face to the original's
    METAFONT (20px) and replaced the eyeballed button rects with the
    `MAINR.BIN` layout (`UI_LAYOUT_IMPORTER`, same pattern as the loading
    screen).
  - `mission/flat_ui_host.rs` — `GuiComponent::Text` now renders at native height
    (vertically centered in the component box) instead of `rect.h`.
- Because alignment is resolved at the seam (`VAlign`), shrinking to native
  height **auto-recenters** text within the existing art-derived rects — no
  per-element position re-tuning needed for the HUD/menu.

### Parser refactor (enables the mandated test)

`dark/src/font.rs` gained `FontMetrics` — the pure, no-GL header+column parse
(previously inlined in `Font::read`, which also builds a GL atlas and so can't
run headlessly). `Font::read` now consumes it. This also fixed an off-by-one
(the old loop read only `num_chars` columns and dropped the last glyph; the
column table has `num_chars + 1` entries). Unit-tested on a synthetic font and
on real MAINFONT.FON / METAFONT.FON (asset-guarded so CI, which lacks game
assets, skips them).

### Inter-glyph spacing (review fix)

The renderer (`SceneObject::screen_space_text`) and the mirrored
`measure_text_width` both added **+1px (canvas) of spacing after every glyph**.
The original's advance is exactly the `.FON` offset-table column difference —
side bearings are baked into the glyph cells, and the shipped fonts carry no
extra spacing. Removed; strings tightened ~8-11% and centering (which goes
through `measure_text_width`) stays consistent by construction.

## Verification

- Unit tests: `FontMetrics` parse (glyph count/widths/height) + `measure_text_width`.
- Visual before/after: HUD and main menu (see PR). BEFORE shows oversized
  "100"/"80"; AFTER renders them at native 11px inside the bio-monitor.
- `RUSTFLAGS="-D warnings" cargo check -p shock2vr -p dark -p engine`.
- SDK missions e2e (all missions still load) + flat-UI e2es (assert labels, not
  pixels — unaffected).

## Follow-ups (not in this PR)

1. **`0xCCCC` AA parser fix**: replace the `alpha > 205 → 0` hack with proper
   palette/coverage normalization (those bytes are palette-resolved coverage;
   in MAINAA only 55/10452 exceed 205, max 209). Distinct from the format-
   `0x0001` 0..15 scaling, which is already exact. Needed before adopting
   BLUEAA/BOLDAA.
2. **HUD/panel face swap to mainaa**: the original's HUD/panel text face is
   **mainaa** (the AA shock font), not numfont. Our current mainfont happens to
   match mainaa's digit ink height (8px), so HUD *size* is already correct —
   the follow-up is only the face swap (do it after the `0xCCCC` normalization
   above so the AA cores render solid). Do not re-touch the HUD sizing path.
3. **Remaining panels**: verify research/keypad MFDs, the log/email reader, and
   save/load screens at native height against original references.
