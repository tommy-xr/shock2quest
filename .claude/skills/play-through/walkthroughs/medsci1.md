# Walkthrough — medsci1 (MedSci Deck 1: Cryo Recovery + Science sector)

Context for the `playtest` agent and the `play-through` reviewer. This is the
**real critical path** (cross-checked against portforward.com, sshock2.com, and
powerpyx/Steam code guides, then verified against `medsci1.mis` entity data) —
use it to play with intent and to judge whether a session genuinely progressed.
It is **not a script**: observe and react; use the anchors to know where the
path goes and what *should* happen there.

**Positions are runtime-space** (same space as `/v1/player/*`, `/v1/entities`)
verified against a live runtime. **Resolve entities by name/template at run
time** (`GET /v1/entities?filter=...`) — never hardcode runtime ids. The
`template_id` in `/v1/entities` output is the stable handle listed below.

## Deck structure

Two floors: **lower** (y ≈ −4…−8: spawn cryo bay, cryo-recovery block, upgrade
hub) and **upper** (y ≈ 0…1: most of the Science sector — elevator lobby,
chemical storeroom, turret area, bulkheads). Vertical traversal is by **ladders
and small lift platforms**, not stairs.

Three exits (live `/v1/transitions`):

| exit | entity | position | leads to |
| --- | --- | --- | --- |
| Medical bulkhead (north) | `Bulk_On_Button` tmpl 1009 | (43.9, 0.0, −76.4) | medsci2 loc 100 |
| Medical bulkhead (south) | `Bulk_On_Button` tmpl 1103 | (45.5, 0.0, −15.6) | medsci2 loc 300 |
| **Maintenance conduit (the level exit)** | `Tripwire` tmpl 764 (TrapTripLevel) | (12.7, −5.6, −43.6) | **eng1** loc 21 |

The deck's **main elevator is inoperative** (Master Elevator Button tmpl 1041 at
(0.4, 0, −40.4), Double Elevator Doors ~(0.8, 0.2, −42)) — it is NOT the exit.
The story objective chain: cryo escape → Science sector → **medsci2 round trip**
(Grassi's Crew card → Watts' office R&D card) → back to medsci1 R&D → Watts'
deck-2/log-14 audio log with code 12451 → maintenance conduit → Engineering.

## Valid campaign start

For end-to-end acceptance, enter `medsci1` through the real Station deployment
flow with the selected career, character upgrades, inventory, and quest state
preserved. Waking in the cryo bay after that transition is the campaign start.

A fresh `--mission medsci1.mis` launch is useful for isolated investigation,
but it does not prove the Earth / Station handoff or campaign persistence.
Likewise, a frontier save may resume already-proven ground, but raw teleport,
`/v1/player/give`, `/v1/quests` mutation, debug entity messages, and direct
level transitions cannot be used to bridge a critical-path gate.

## Codes & required items

| Item / code | Where | Unlocks |
| --- | --- | --- |
| Wrench (tmpl 990) | **inside** MS Male Corpse (tmpl 1177) @ (−37.3, −5.5, 31.8), cryo bay near spawn — frob the corpse, click the Wrench in the loot panel (`/v1/ui`) | smash debris blocking the first ladder; melee |
| **Code 45100** | Amanpour audio log, mission id 1608 (deck 2 / log 20), inside MS Male Corpse id 1680 @ (−31.3, −1.1, −9.5); loot it through the corpse MFD and visibly read 45100 in its transcript | cryo-recovery exit door (keypad tmpl 1681 @ (−26.5, 0.2, −12.7)) |
| Cryo Card (tmpl 1050) @ (−24.3, 0.0, −1.4) | upper walkway after the first ladder | first card-reader door |
| Power cell #1: Dead Power Cell tmpl 1767 @ (−13.8, −6.5, −34.3) | near jammed door; charge at Recharging Station tmpl 125 @ (−15.8, −6.0, −27.9) | Aux Power receptor tmpl 1766 @ (−13.1, −5.8, −33.1) → jammed cryo-escape door. (Spare Wrench tmpl 1707 @ (−18.5, −6.7, −29.3) here.) |
| Science Card (tmpl 338) @ (−16.6, 1.2, −68.9) | corpse near the "ghost" (upgrade-hub exit) | card door into the Science sector proper |
| Power cell #2: Dead Power Cell tmpl 1186 @ (29.8, −1.1, −75.7) | reception near the "MED" sign (Polito points it out) | Aux Power receptor tmpl 1128 @ (32.1, 0.9, −76.8) → powers the **north Medical bulkhead** (→ medsci2) |
| **Code 12451** | Watts' deck-2/log-14 audio log, on Watts in **medsci1 R&D** after returning from medsci2 with the R&D card | maintenance conduit keypad tmpl 809 @ (6.3, 0.4, −47.4) → the eng1 exit |
| Optional: code 00000 (cryo closet, BrawnBoost); code 98383 (sub-armory, medsci2 side) | | |

## Ordered objective snapshots

Record `/v1/quests` at these boundaries. `incomplete` (raw bit 1) means an
objective / marker is active; `complete` means it has been resolved. These
snapshots supplement physical evidence—they do not replace the interaction:

| after genuine interaction | expected objective evidence |
| --- | --- |
| campaign arrival / early cryo | `Note_2_2` is the active secure-airlock objective; record its baseline and its completion after the airlock path |
| charged cell #2 inserted into Aux Power 1128 | `Note_2_10=complete` (direct mission SwitchLink); `Note_2_9` is the Grassi/Crew-card objective |
| Crew Card looted from Grassi in MedSci2 | `grassicard=incomplete` (raw 1) and `Note_2_9=complete` |
| R&D Card looted from Watts' Crew office | `RDGrab=incomplete` (raw 1) and `Note_2_5=complete` |
| physical Watts encounter in MedSci1 R&D | `Note_2_3=complete` |
| Watts' deck-2/log-14 disc genuinely acquired | `wattsy=incomplete` (raw 1), `Note_2_6=complete`, and `Note_2_7=incomplete` (the newly learned 12451 note) |
| 12451 accepted by maintenance keypad 809 | `Note_2_7=complete`; `Note_2_1` is the continuing goal to reach Deck 4 |

The card and Watts marker rows are driven by the mission's `FrobQB` /
Simple-QB-trigger chains. If the in-world action succeeds visually but its
ordered objective snapshot does not advance, report the objective integration
failure rather than setting the bit by hand.

## Critical path (granular)

### Phase A — Cryo Recovery escape (lower floor, around spawn)
1. **Wake** at (−35, −4.6, 17.9); Polito radios that the section is breached.
2. **Loot the Wrench** (tmpl 990) from the MS Male Corpse (tmpl 1177,
   (−37.3, −5.5, 31.8)): frob the corpse → its loot MFD opens → click the
   Wrench element. Contained loot is never world-placed on the floor.
3. **Smash the debris and climb the ladder** behind the fallen Air Duct (tmpl
   402, (−41.6, −3.9, 17.9)); ladder rungs `Rick Ladder` stacked at
   (−41.3, −5.8…−2.0, 16.5) → up to the walkway (y ≈ 0).
4. **Pick up the Cryo Card** (tmpl 1050, (−24.3, 0.0, −1.4)); use it on the
   card-reader door.
5. **First corpse: Amanpour audio log id 1608** inside corpse id 1680. Frob the
   corpse, click the contained disc, and visibly read **45100** in its deck-2 /
   log-20 transcript; then enter it on **keypad tmpl 1681**
   (−26.5, 0.2, −12.7) to open the cryo-recovery exit door.
6. **Air-shaft crawl** section (low-clearance passage), leads down toward the
   jammed-door area.
7. **Power cell puzzle #1**: take Dead Power Cell (tmpl 1767), charge it at the
   Recharging Station (tmpl 125), insert into Aux Power (tmpl 1766) — the
   jammed door opens.
8. Continue through the real secure-airlock route into EmailRoom **`free1`**
   (entity 1408). Require the objective activated by EmailTrap 1131 to change
   from `Note_2_2=incomplete` to `Note_2_2=complete` when the authored room is
   entered; hearing an email without the quest transition is not completion.
9. **Cyber-upgrade hub**: Polito awards modules; 4 upgrade stations. (Optional:
   cryo closet keypad 00000 nearby.)
10. **Ride the lift up** out of the lower level (Elevator Path pair tmpl 262/263:
   (−27.6, −7.9, −54.5) → (−27.6, −1.7, −54.5)).
11. **Take the Science Card** (tmpl 338, (−16.6, 1.2, −68.9)) from the corpse
    near the "ghost"; it opens the card door out.
11a. **Detailed ladder detour (not mission critical):** after using the Science
    Card to enter the upper Science walk network, follow the R&D sign /
    Watts-window corridor to the **top** landing near (−18.0, −0.4, 13.0).
    Descend the six-rung `Rick Ladder` stack at
    (−17.5, −6.0…−2.0, 14.5), inspect or clear the lower hybrid room, then
    approach the ladder's north face from z ≈ 15.8 and climb back to the upper
    corridor. This room has no lower door or pre-airshaft entrance; the ladder
    itself is its authored access, so do not search for it during early Cryo
    Recovery or before acquiring the Science Card.
12. **First combat: 2 hybrids** past that door. Then a second platform ride up
    to the elevator lobby.

### Phase B — Science sector → Medical bulkhead (upper floor)
13. **Elevator lobby** (~(0.4, 0, −40)): pressing the elevator button triggers
    Polito's "elevator is inoperative" objective chain. The **maintenance
    conduit door** (keypad tmpl 809, (6.3, 0.4, −47.4); "To Maintenence Shaft"
    sign tmpl 1281 at (5.4, −1.6, −49.1)) is here but **locked — code 12451 not
    known yet**. Security cameras (5 on the deck) start appearing.
14. **Power cell #2**: scripted explosion near the "MED" sign; take Dead Power
    Cell (tmpl 1186, (29.8, −1.1, −75.7)) from reception.
15. Pass the Xerxes computer room / bio-reconstruction room (optional: activate
    the Quantum Bio-Reconstruction machine; pistol near the info kiosk).
16. **Turret gauntlet**: catwalk over a room with 2 Slug Turrets (turrets in
    data: tmpl 610 (3.2, −5.2, −14.0), tmpl 611 (−4.7, −5.3, −6.1), tmpl 1015
    (18.8, −0.4, 9.5)). Recharge the power cell at a Recharging Station
    (tmpl 506 (−6.8, −4.0, −13.1) or tmpl 273 (10.5, −2.8, 33.0)).
17. **Insert the charged cell into Aux Power tmpl 1128** (32.1, 0.9, −76.8) →
    the north Medical bulkhead powers up; hit `Bulk_On_Button` tmpl 1009 →
    **level transition to medsci2**.

### Phase C — medsci2 excursion (required; separate walkthrough)
18. Medical sector → Grassi's corpse (Crew access card) → Crew Quarters →
    Watts' office desk (R&D card) → return through the bulkhead to medsci1 →
    use the R&D card to enter medsci1 R&D → meet Watts and loot/read his
    **deck-2/log-14 audio log with code 12451**.

### Phase D — Exit to Engineering
19. Back at the conduit: **enter 12451 on keypad tmpl 809** — door opens (the
    keypad SwitchLinks the door + an Experience Trap: cyber-module award).
20. **Climb DOWN the shaft ladder** (`Rick Ladder 16` tmpl 1140 at
    (12.6, −5.6, −42.7)) — one Blue Monkey may be around the conduit.
21. At the bottom, the **Tripwire (tmpl 764) fires → eng1.mis** (loc 21, near
    Engineering Control). medsci1 done.

## Acceptance checklist — what counts as genuine play

A MedSci1 pass is valid only if the session evidence shows all of:

- Campaign entry from Station (or a documented frontier resuming already-proven
  play), with character and quest state preserved.
- Wrench looted from its corpse, blocking debris broken, and the early Cryo
  ladder traversed through normal player movement.
- For a detailed pass, the later R&D-overlook ladder detour entered from its
  upper landing, descended into the lower hybrid room, and climbed back out.
  This is not an early or pre-airshaft gate and is not required by the mission's
  critical path.
- Cryo Card acquired and used at its real reader.
- Corpse id 1680 opened, Amanpour disc id 1608 clicked through the loot MFD,
  deck 2 / log 20 recorded in `player.collected_logs`, and the in-game reader
  visibly showing **45100** before that code is entered on keypad 1681.
- The air-shaft crawl genuinely traversed; Power Cell #1 carried, charged, and
  inserted; the secure-airlock objective activated and then completed by
  physically entering `free1` / entity 1408; both lift platforms ridden;
  Science Card acquired and used at its actual reader.
- Power Cell #2 carried from reception, charged, and inserted into Aux Power
  1128, with `Note_2_10=complete`, followed by the real north Medical bulkhead
  transition.
- The complete MedSci2 loop proven under `medsci2.md`: Grassi's Crew Card,
  Crew reader/door, Watts-office desk R&D Card, and a real return bulkhead, with
  card and objective state surviving back into MedSci1.
- The R&D Card used at the MedSci1 reader; physical Watts reached and frobbed;
  deck-2/log-14 taken from his loot MFD; visible transcript showing **12451**;
  and `wattsy`, `Note_2_6`, and `Note_2_7` advancing in the expected order.
- **12451** entered through the real maintenance keypad 809, the conduit door
  crossed, the shaft ladder descended, and the bottom tripwire physically
  transitioning the campaign to **`eng1.mis` loc 21**.

Codes remembered from a walkthrough, source file, mission property, or previous
run are not valid conveyance. Typing 45100 without reading Amanpour's disc, or
12451 without taking Watts' disc and advancing its objective chain, invalidates
the playthrough even if the keypad opens. Debug-given items, injected quest
bits, direct door messages, debug damage in place of combat, and direct level
transitions are likewise insufficient.

If EmailTrap 1131 presents the secure-airlock objective but genuine entry into
`free1` / entity 1408 does not complete `Note_2_2`, classify it as a campaign
objective-metadata feature gap. `TrapEmail` presentation or downstream
traversal alone must not be used to waive the missing quest transition.

## Required non-walk mechanics — engine watch-points

Pure walk + frob does **NOT** finish this level. Each of these is required and
is a potential feature-gap finding for the play-through loop — test them
honestly (report a blocker rather than teleporting past):

1. **Ladder climbing** (steps 3, 20) — ladders have `PropPhysAttr.climbable: 27`.
   Flat (desktop-style) climbing is implemented as of 2026-07: push toward the
   ladder to ascend, look down + push to descend. Known gaps: entering a
   descent from the top lip is untested (the grip needs horizontal overlap),
   and VR grip-climbing is still unimplemented (`projects/climbing.md`).
2. **Breakable debris** (step 3) — wrench-smash to clear the ladder approach.
3. **Air-shaft crawl** (step 6) — low-clearance traversal (crouch).
4. **Power-cell carry + insert** (steps 7, 16) — pick up, recharge, socket into
   an Aux Power receptor.
5. **Lift platform rides** (steps 9, 11) — moving-terrain elevators
   (`Elevator Path` pairs; the path DB shows `moving_terrain` cells).
6. **Keypad code entry** (steps 5, 18) — 45100 / 12451 via the keypad UI.
7. **Bulkhead round trip to medsci2 and back** (steps 16–17) — cross-mission
   state (inventory + quest bits must survive both transitions).

Also known: the AIPATH walk-graph does NOT connect spawn → exit (verified with
`cargo bn path show`, even with permissive movement bits) — consistent with the
ladder/lift/crawl legs above splitting the walk components. Any future
"navigate by pathfinding" upgrade must handle those seams.

## Sources

- portforward.com SS2 walkthrough (Science / Crew-Quarters / Medical pages)
- sshock2.com full walkthrough (deck split, codes/cards)
- powerpyx + Steam access-code guides (codes 45100 / 00000 / 12451 / 98383)
- `cargo dq entities medsci1.mis ...` + live debug-runtime entity dump
  (2026-07-09) for positions/ids; tripwire + keypad SwitchLinks verified in
  mission data.
