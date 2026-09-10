# Astra MFD controls and dual weapons

The unused regions have retail functions worth restoring. Keep their recognizable controls and reserve the weapon side for independently bound left/right weapon panels. The center is useful world-view space; filling it is not itself a goal.

Retail behavior below is confirmed by the [System Shock 2 manual, PDF page 5](https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/238210/manuals/System%20Shock%202%20-%20Manual.pdf). Repository status is from the current implementation, not a claim of complete retail parity.

| Control | Retail purpose | Existing foundation / missing work |
| --- | --- | --- |
| Research (test tube) | Current research status and completed reports | `ResearchState` tracks progress, chemical needs and completion; `ResearchGui` shows an item's status/report. Add a persistent research overview and entry button, including when the original object is no longer carried. Opening the overview must not start or restart research. |
| Item information (?) | Select the button, then an item to read its description | The host already resolves hovered/lifted inventory items; `PropObjLookString` is parsed and used for research report fallback. Add a read-only inspection screen and explicit inspect interaction. No inventory Frob/use effects: inspecting a hypo must never consume it. |
| Map | Open the current deck map | `Effect::ToggleMap` and the map MFD exist; expose the shared action through the matching button. |
| PDA | Communications, logs and notes | Log collection/reader and media UI exist. Reuse them, then audit email/notes/help coverage separately; a last-unread action is not the full PDA browser. |
| Access cards | List collected clearances | `QuestInfo` stores acquired key cards. Add a read-only clearance list; do not create physical duplicate cards. |
| Character MFD | Statistics and abilities | `PlayerStats` and trainer/psi screens exist. Add a read-only character summary without exposing trainer purchase actions. |
| Nanites / upgrade points | Persistent resource totals | Display actual player-state values beside the corresponding icons, including an explicit zero. |

## Implementation order

1. Complete the current wrist corrections: bracelet bio orientation and intact compact ammo art.
2. Build the MFD composition around retail utility slots and two named weapon panels. Read authored BIN layout rectangles before placing controls; verify the same canvas in flat and VR.
3. Resolve the held entity once per weapon panel. Bind reload, ammo cycling and settings to it, and revalidate that it is still held before activating. Support hands never create a duplicate weapon panel. Flat still has one wielded weapon.
4. Wire Map and Research entry points, then item information. The research entry opens status without mutating research; the question-mark mode consumes only the inspection click and has an obvious cancel/exit.
5. Add access-card and character summaries, then finish the PDA browser gaps.

## Review cases

### Item information control

The `?` utility enters a selection mode that consumes click/grab edges before
inventory use, wield, drop or Frob. Select an inventory item, cursor item, or
held-item arm readout to open its localized description in the right MFD.
`?` again cancels selection; CLOSE dismisses the reader. Long descriptions
have previous/next pages. Closing the cyber interface resets inspection.
Ordinary items use their object-name key in OBJLOOKS; an authored look-string
override takes priority. Missing entries show an explicit fallback, never the
short item name masquerading as a description.

The vial, `?`, and MAP buttons use the native BIOFULL wells and IFBTN40,
IFBTN30, and IFBTN50 artwork. Their drawing, input and debug rectangles share
`shkiface.cpp`'s authored canvas coordinates. AMMOFULL stays visible on the
right with empty hands; its weapon controls appear only with relevant content.
Clicking either strip while carrying a cursor item preserves the item.

BIOFULL's two resource wells show nanites (`nan_ic`) and cyber modules
(`upgrade`) with live balances. The nanite total is the same one used by
purchases, including legacy carried stacks. The first AMMOFULL well holds the
log disc button: it opens the existing latest-unread collected-log reader,
falling back to the latest readable log. Pressing it again closes just that
reader and leaves the cyber interface open. With no collected logs it shows
an empty native PDA panel. This is a shortcut, not yet a full PDA log browser.

Both inventory arms show their own ammo count (or PSI for an amp). Selecting
an armed slot with an empty cursor selects that weapon for AMMOFULL; its arm
gets a green underline. Selection
follows the same weapon across a hand swap. Putting it away selects another
available gun, preferring the physical right hand when no prior selection remains.
A support hand owns no item and adds no duplicate counter.

Reload, ammo cycling and settings carry the entity from the last drawn panel.
A dropped/stale target does nothing; it cannot redirect a queued click to the
other gun. Keyboard actions retain their existing right-first preference.
Weapon settings bind to an explicit gun and close when selection changes.
Selecting arms never equips, consumes, drops or moves inventory items.

### Research overview

The vial opens the retail PDA-style research list in the left MFD. Active projects
open the RESEARCH status layout; completed entries open RESREP with the
portrait, flask icon and report text from RESEARCH.STR, not OBJLOOKS. Reports
are selected from collected report bits and survive the original specimen.
Opening or paging the journal never changes research or consumes chemicals.
The live specimen panel exposes REPORTS and SUSPEND through normal effects.

Layout references are `shkrsrch.cpp`, `shkpda.cpp`, and `shkemail.cpp` in the
original Dark engine: the progress well is (15,267), the specimen slot is
(15,14,138,109), and report body starts at (15,105). Retail MAINAA is loaded
explicitly from `fonts/` and tinted cyan; the alternate `iface/fonts` copy is
not suitable. Specimens currently use their inventory artwork in the preview;
a rotating 3D specimen is still a separate rendering increment.

### Map control

MAP toggles the existing automap in the cyber interface through `ToggleMap`.
Both presentations use its mission map art, explored regions and player pip.
The wide panel scales uniformly to clear the bottom HUD and keep CLOSE
inside the canvas. VR's map is a synthetic cyber-panel host, never an extra
world quad even with the legacy experimental GUI enabled. Opening MAP closes
the utility reader; clicking it again or using the map's close button dismisses
the map. The VR shortcut only acts while the cyber interface is active.

Two different guns; gun plus psi amp; swapped hands; one support grip; no weapon; dropping/swapping a weapon between drawing and clicking its control. Research active, chemical-paused and completed after the item is gone. Inspect a hypo without consumption, a gun without firing, and an ordinary object without research data. Use the same rendered/hit-test rectangles in flat and VR, and retain press-edge protection when opening or switching panels.


### Mirrored arm layout prototype

The inventory now crops the existing arm from `invback.pcx` at runtime and
mirrors it horizontally on the opposite side of the torso. No extracted game
texture is shipped. The original strip scales uniformly by about 0.945 to fit
both arms within the existing canvas width; inventory drop coordinates map
back into the original grid. Flat and VR use the same layout.

### Held-item readouts

Both arms now carry LEFT / RIGHT labels and a fitted icon for the item that
hand owns, or EMPTY. The paperdoll faces the viewer: RIGHT is left of the torso
and LEFT is right of it. Hover names include the hand and full item name;
missing icons fall back to ITEM. The flat viewmodel maps to RIGHT through the
interaction's physical handedness, even though its internal storage uses the
left slot.

The empty caption uses an explicit compact font size so it fits without an
ellipsis. Adjacent background crops share a depth plane in VR; only overlapping
resolved elements advance toward the viewer to preserve painter order. This
avoids introducing a perspective seam between pieces of the same border.

The arms are read-only: clicking them does not equip, use, or throw an item.
Armor, implant and inventory controls retain their existing behavior. These
readouts describe held items independently of shoulder recall assignments.
Opening the cyber interface currently releases a support grip for pointing,
so the freed hand reads EMPTY and a two-handed weapon appears only once. If
that interaction policy changes, add an explicit SUPPORTING state rather than
representing the support hand as another owner.

### Character MFD

The native MFD control (IFBTN20/21, at 460,430) toggles a read-only character
sheet in the original right STATS panel. It shows the five trained base levels
in retail order and acquired OS upgrade icons/names. Bars use shkstats.cpp's
33,22 origin, 17-pixel horizontal step and 26-pixel row step; acquired traits
pack left-to-right at 35-pixel spacing. Label bands are replaced consistently
in classic and 25AE artwork so the HD art's removed labels remain readable.

This first sheet reports base levels, not temporary boosted-stat segments.
Upgrade purchases remain at trainers. TECH/CMBT/PSI pages and their navigation
controls are deferred; no inactive tabs are presented as working controls.
