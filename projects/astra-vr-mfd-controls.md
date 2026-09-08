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

Two different guns; gun plus psi amp; swapped hands; one support grip; no weapon; dropping/swapping a weapon between drawing and clicking its control. Research active, chemical-paused and completed after the item is gone. Inspect a hypo without consumption, a gun without firing, and an ordinary object without research data. Use the same rendered/hit-test rectangles in flat and VR, and retain press-edge protection when opening or switching panels.

A weapon-pinned ammo display remains a separate optional grip-editor placement mode. The psi amp keeps its authored forearm and needs its own mount; glove-mounted wrist plates deliberately skip it.
