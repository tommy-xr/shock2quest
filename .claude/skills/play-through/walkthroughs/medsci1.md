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
(Grassi → Watts, who has code 12451) → back → maintenance conduit → Engineering.

## Codes & required items

| Item / code | Where | Unlocks |
| --- | --- | --- |
| Wrench (tmpl 990) @ (−38.1, −5.9, 29.8) | cryo bay floor near spawn | smash debris blocking the first ladder; melee |
| **Code 45100** | audio log (Amanpour) on first corpse past the card door | cryo-recovery exit door (keypad tmpl 1681 @ (−26.5, 0.2, −12.7)) |
| Cryo Card (tmpl 1050) @ (−24.3, 0.0, −1.4) | upper walkway after the first ladder | first card-reader door |
| Power cell #1: Dead Power Cell tmpl 1767 @ (−13.8, −6.5, −34.3) | near jammed door; charge at Recharging Station tmpl 125 @ (−15.8, −6.0, −27.9) | Aux Power receptor tmpl 1766 @ (−13.1, −5.8, −33.1) → jammed cryo-escape door. (Spare Wrench tmpl 1707 @ (−18.5, −6.7, −29.3) here.) |
| Science Card (tmpl 338) @ (−16.6, 1.2, −68.9) | corpse near the "ghost" (upgrade-hub exit) | card door into the Science sector proper |
| Power cell #2: Dead Power Cell tmpl 1186 @ (29.8, −1.1, −75.7) | reception near the "MED" sign (Polito points it out) | Aux Power receptor tmpl 1128 @ (32.1, 0.9, −76.8) → powers the **north Medical bulkhead** (→ medsci2) |
| **Code 12451** | Watts' audio log, R&D — **in medsci2** | maintenance conduit keypad tmpl 809 @ (6.3, 0.4, −47.4) → the eng1 exit |
| Optional: code 00000 (cryo closet, BrawnBoost); code 98383 (sub-armory, medsci2 side) | | |

## Critical path (granular)

### Phase A — Cryo Recovery escape (lower floor, around spawn)
1. **Wake** at (−35, −4.6, 17.9); Polito radios that the section is breached.
2. **Take the Wrench** (tmpl 990, (−38.1, −5.9, 29.8)) from the bay floor.
3. **Smash the debris and climb the ladder** behind the fallen Air Duct (tmpl
   402, (−41.6, −3.9, 17.9)); ladder rungs `Rick Ladder` stacked at
   (−41.3, −5.8…−2.0, 16.5) → up to the walkway (y ≈ 0). *(A second required
   ladder stack sits at (−17.5, −6.0…−2.0, 14.5).)*
4. **Pick up the Cryo Card** (tmpl 1050, (−24.3, 0.0, −1.4)); use it on the
   card-reader door.
5. **First corpse: audio log with code 45100**; enter it on **keypad tmpl 1681**
   (−26.5, 0.2, −12.7) to open the cryo-recovery exit door.
6. **Air-shaft crawl** section (low-clearance passage), leads down toward the
   jammed-door area.
7. **Power cell puzzle #1**: take Dead Power Cell (tmpl 1767), charge it at the
   Recharging Station (tmpl 125), insert into Aux Power (tmpl 1766) — the
   jammed door opens.
8. **Cyber-upgrade hub**: Polito awards modules; 4 upgrade stations. (Optional:
   cryo closet keypad 00000 nearby.)
9. **Ride the lift up** out of the lower level (Elevator Path pair tmpl 262/263:
   (−27.6, −7.9, −54.5) → (−27.6, −1.7, −54.5)).
10. **Take the Science Card** (tmpl 338, (−16.6, 1.2, −68.9)) from the corpse
    near the "ghost"; it opens the card door out.
11. **First combat: 2 hybrids** past that door. Then a second platform ride up
    to the elevator lobby.

### Phase B — Science sector → Medical bulkhead (upper floor)
12. **Elevator lobby** (~(0.4, 0, −40)): pressing the elevator button triggers
    Polito's "elevator is inoperative" objective chain. The **maintenance
    conduit door** (keypad tmpl 809, (6.3, 0.4, −47.4); "To Maintenence Shaft"
    sign tmpl 1281 at (5.4, −1.6, −49.1)) is here but **locked — code 12451 not
    known yet**. Security cameras (5 on the deck) start appearing.
13. **Power cell #2**: scripted explosion near the "MED" sign; take Dead Power
    Cell (tmpl 1186, (29.8, −1.1, −75.7)) from reception.
14. Pass the Xerxes computer room / bio-reconstruction room (optional: activate
    the Quantum Bio-Reconstruction machine; pistol near the info kiosk).
15. **Turret gauntlet**: catwalk over a room with 2 Slug Turrets (turrets in
    data: tmpl 610 (3.2, −5.2, −14.0), tmpl 611 (−4.7, −5.3, −6.1), tmpl 1015
    (18.8, −0.4, 9.5)). Recharge the power cell at a Recharging Station
    (tmpl 506 (−6.8, −4.0, −13.1) or tmpl 273 (10.5, −2.8, 33.0)).
16. **Insert the charged cell into Aux Power tmpl 1128** (32.1, 0.9, −76.8) →
    the north Medical bulkhead powers up; hit `Bulk_On_Button` tmpl 1009 →
    **level transition to medsci2**.

### Phase C — medsci2 excursion (required; separate walkthrough)
17. Medical sector → Grassi's corpse (Crew access card) → Crew Quarters →
    Watts' office (R&D card) → **Watts' audio log with code 12451** → return
    through the bulkhead to medsci1.

### Phase D — Exit to Engineering
18. Back at the conduit: **enter 12451 on keypad tmpl 809** — door opens (the
    keypad SwitchLinks the door + an Experience Trap: cyber-module award).
19. **Climb DOWN the shaft ladder** (`Rick Ladder 16` tmpl 1140 at
    (12.6, −5.6, −42.7)) — one Blue Monkey may be around the conduit.
20. At the bottom, the **Tripwire (tmpl 764) fires → eng1.mis** (loc 21, near
    Engineering Control). medsci1 done.

## Required non-walk mechanics — engine watch-points

Pure walk + frob does **NOT** finish this level. Each of these is required and
is a potential feature-gap finding for the play-through loop — test them
honestly (report a blocker rather than teleporting past):

1. **Ladder climbing** (steps 3, 19) — ladders have `PropPhysAttr.climbable: 27`;
   as of 2026-07 climbing is **unimplemented** in shock2vr (`projects/climbing.md`
   is a plan). Expected FIRST blocker of an honest run.
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
