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

- **Fonts**: `dark/src/font.rs` parses Dark `.FON` bitmap fonts (both mono
  `format=0` and anti-aliased `format=0xCCCC`) into a glyph atlas + per-glyph
  advance/`base_height` (native pixel height). Exposed through the
  `engine::Font` trait and `dark::importers::FONT_IMPORTER`.
- **Primitive**: `SceneObject::screen_space_text` (`engine/src/scene/scene_object.rs`)
  lays out glyphs at `font_size` px; `multiplier = font_size / base_height`.
- **Canvas**: `shock2vr/src/ui/mod.rs` (`UiCanvas`) describes UI in **640×480
  canvas pixels**; `render_screen_space` maps canvas→screen (`scale`, letterbox)
  and draws each `Text` element via `screen_space_text` with
  `font_size = size * scale.y`. Alignment (`HAlign`/`VAlign`) is resolved here.

So `size` on a canvas `Text` element **is** the glyph pixel height on the
640×480 canvas. The seam is correct; the callers passed the wrong `size`.

### Fonts and their native heights

Decoded from `res/fonts/*.FON` (header: `format@0`, `first@0x24`, `last@0x26`,
`width_offset@0x48`, `bitmap_offset@0x4c`, `row_width@0x50`, `num_rows@0x52`):

| Font          | format | native height (px) | notes                        |
| ------------- | ------ | ------------------ | ---------------------------- |
| MAINFONT.FON  | 0 mono | **11**             | the general UI/HUD font we use |
| MAINAA.FON    | 0xCCCC | **12**             | anti-aliased menu font        |
| BOLDAA.FON    | 0xCCCC | 14                 | (unused today)                |
| BLUEAA.FON    | 0xCCCC | 20                 | (unused today)                |
| DIMMED.FON    | 0xCCCC | 12                 | (unused today)                |
| bignum.fon    | 0 mono | 22                 | big numerics (unused today)   |
| numfont.fon   | 0xCCCC | 11                 | HUD numerics (unused today)   |
| keyfont/KEYFONTA | -   | 22                 | (unused today)                |

Only **mainfont.fon** (13 call sites) and **mainaa.fon** (menu) are referenced
in code today.

## Gap table (element → original vs. ours before the fix)

| Element                    | Font (native px) | Our size (before) | Delta        |
| -------------------------- | ---------------- | ----------------- | ------------ |
| Main-menu buttons          | mainaa (12)      | 19                | ~1.6× big    |
| HUD health/PSI numbers     | mainfont (11)    | 16                | ~1.45× big   |
| HUD ammo count             | mainfont (11)    | 18                | ~1.6× big    |
| HUD ammo-type label        | mainfont (11)    | 12                | ~1.1× big    |
| HUD PSI power name         | mainfont (11)    | 10                | ~0.9× (ok)   |
| Panel text (elevator, MFD) | mainfont (11)    | **= rect height** (e.g. 20) | ~1.8×, and coupled to the box size — a plain bug |

The panel path was the worst offender: `flat_ui_host` drew `GuiComponent::Text`
with `size = rect.h` (the component's *bounding-box* height, not a font size), so
a 20px-tall floor-label box rendered 20px text.

## Root cause

Two independent issues; **(1) dominates**:

1. **Sizing**: UI text was rendered at ad-hoc `size` constants larger than the
   font's native pixel height. Bitmap fonts must render at native height (1:1)
   to match the original. This is the "way off across the whole flat UI"
   complaint.
2. **AA glyph parsing (minor, deferred)**: the `.FON` parser's anti-aliased
   branch has `if alpha > 205 { alpha = 0 }`, which *erases* the densest (solid)
   glyph pixels — AA coverage tops out at ~210, not 255. It hollows AA glyph
   cores. Barely affects MAINAA (only ~55 px), so the menu still looks fine, but
   it corrupts fonts like BLUEAA (~1014 px). Only matters once we adopt more AA
   fonts. **Left as a follow-up** (see below).

## The fix (this PR)

Route the correction through the seam so it generalizes:

- `UiCanvas::text` now treats `size <= 0.0` as a sentinel meaning **render at the
  font's native `base_height`** (resolved at render time, where the font is
  loaded). New convenience `UiCanvas::text_native(...)` is the fidelity-correct
  default.
- Migrated the three flat-UI text paths to native sizing:
  - `hud/flat_hud.rs` — all HUD text (`text_native`); removed the ad-hoc size
    constants.
  - `scenes/main_menu.rs` — menu labels (`text_native`, mainaa 12px); removed
    `MENU_FONT_SIZE`.
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
on real MAINFONT.FON (asset-guarded so CI, which lacks game assets, skips it).

## Verification

- Unit tests: `FontMetrics` parse (glyph count/widths/height) + `measure_text_width`.
- Visual before/after: HUD and main menu (see PR). BEFORE shows oversized
  "100"/"80"; AFTER renders them at native 11px inside the bio-monitor.
- `RUSTFLAGS="-D warnings" cargo check -p shock2vr -p dark -p engine`.
- SDK missions e2e (all missions still load) + flat-UI e2es (assert labels, not
  pixels — unaffected).

## Follow-ups (not in this PR)

1. **AA parser fix**: replace the `alpha > 205 → 0` hack with proper coverage
   normalization (scale 0..~210 → 0..255) so AA glyph cores render solid. Needed
   before adopting BLUEAA/BOLDAA/numfont.
2. **Per-font fidelity**: the original uses distinct fonts per element (numfont
   for HUD numerics, bignum for large counts). We use mainfont everywhere. Once
   the AA parser is fixed, swap in the correct faces.
3. **Remaining panels**: verify research/keypad MFDs, the log/email reader, and
   save/load screens at native height against original references.
