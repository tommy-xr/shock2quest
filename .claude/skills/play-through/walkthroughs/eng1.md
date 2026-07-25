# Walkthrough — eng1 + eng2 (Engineering: restore main power)

Context for the `playtest` agent and the `play-through` reviewer. This is the
**story-critical Engineering path**, cross-checked against the retail objective
strings, external walkthroughs, and `eng1.mis` / `eng2.mis` entity wiring. It is
not enough to cross the eng1↔eng2 bulkhead or to make an elevator transition:
Engineering is complete only after the player has performed the Fluidics,
nacelle, and master-power sequence through the real in-world objects.

This is not a movement script. Observe and react, traverse with bounded
`/v1/player/move`, fight or evade threats, and use the anchors below to identify
the intended objective rather than teleporting between coordinates.

Positions and positive IDs below are mission-file object identities from
`cargo dq`; resolve runtime entities anew by name and stable `template_id` each
launch. Never retain a concrete runtime entity ID. The arrival coordinates from
the earlier campaign seed are retained where useful.

## What Engineering actually is

- `eng1.mis` is Engineering A: coolant tubes, Engineering/Fluidics Control,
  Auxiliary Storage 4/5, Engine Core, both nacelles, the master-power room, the
  main deck elevator, and the Security Station.
- `eng2.mis` is Engineering B: Command Control and Cargo Bays 1A/1B/2A/2B.
- The bulkhead between them is a **routine round-trip connector**, not an
  objective completion point.
- The Security access card and Utility Storage 4 supplies/hazard suit are useful
  but optional. They are not the narrative gate.
- The keypad `94834` in the Engine Core is for the much later self-destruct
  return to Engineering. Do not use it during the first visit.
- Completing the first visit sends the player to the main elevator and
  **Hydroponics (deck 3)**. In this port the main elevator's Hydro button loads
  `hydro2.mis` at loc 22, not `hydro1.mis`.

## Arrival, connectors, and major anchors

| Purpose | Stable mission object | Position | Expected behavior |
| --- | --- | --- | --- |
| Arrival from MedSci conduit | eng1 arrival | about (44.6, −15.2, −40.4) | Engineering Control alcove; protective-clothing sign and nearby log/desk |
| Return to MedSci | `Tripwire Level` 463 | (44.70, −10.24, −33.90) | `MedSci1`, loc 12 |
| eng1 → eng2 | `Bulk_On_Button` 1124 | (2.7, −11.1, −198.2) | Frob transitions to `eng2`, loc 100 |
| eng2 → eng1 | `Bulk_On_Button` 1080 | (−2.7, −11.1, −192.2) | Frob transitions to `eng1`, loc 100 |
| Main elevator panel | `Master Elevator Button` 629 | (−9.61, −15.09, −111.38) | Opens the five-deck elevator UI |
| Locked elevator-door control | `Elevator Button` 990 | (−8.75, −14.77, −116.78) | Initially locked; master-power router unlocks it |

The eng1→eng2 button was successfully frobbed in campaign iteration 6. That
proved the connector only; the session then bridged the interior and therefore
did **not** validate Engineering's objectives.

## Required items, codes, and evidence

| Item / code | Genuine source and location | Purpose / evidence |
| --- | --- | --- |
| Engineering Control direction | Sanger log 13, entity 451, by the Eng Control door at (0.78, −2.37, −166.92) | Read its in-game transcript: it says she went to Cargo Bay 2 and should activate `Note_1_6` |
| Cargo Bay 2A/2B access card | entity 520, **inside corpse 233** in Cargo Bay 1A at (−33.31, −12.32, −282.73), eng2 | Card region `16384`; opens the Cargo Bay 2 reader/doors |
| Code **15061** | Sanger's “Locked in” log 5, entity 1426, **inside corpse 507** on the upper level of Cargo Bay 2B at (31.61, 2.17, −172.71), eng2 | Read its in-game transcript before using the code; it advances the objective from finding the code (`Note_1_6`) to knowing it (`Note_1_8`) |
| Engineering Control keypad | keypad 1181 at (−1.59, −0.60, −168.60), eng1 | Code 15061; opens door 1091 and completes `Note_1_6` + `Note_1_8` |
| Fluidics-backdoor instructions | Delacroix log 7, entity 1421 at (1.80, −2.75, −177.29), eng1 | Read its in-game transcript: it names 45m/dEx, Command Control, Aux Storage 5, and code 34760, and advances the override objective |
| Code **34760** | learned from Delacroix log 7 in-game | Opens Aux Storage 5 keypad 1726 at (−41.12, −14.97, −53.95); typing it from this document is not discovery coverage |
| Hardware override 45m/dEx | circuit board entity 705 (`obj:engcard5`, displayed as “Part #45M/dEX”) at (−52.30, −14.92, −51.09), eng1 | Picking it up completes `Note_1_10`; must survive the transition to eng2 |
| Systems Monitoring Unit | `Card_ Box` 676 at (0.00, 9.89, −290.06), Command Control, eng2 | `ObjConsumeButton`, consumes a `Circuitboard`; sets `CircuitBoardPlaced` and completes `Note_1_9` |
| Optional supplies | keypad 1372 at (−1.81, −14.94, −94.55), code **59004** | Utility Storage 4; hazard suit/supplies, completes `Note_1_11` |
| Optional Security access | Security Card 901 inside Male Corpse 1331 near (−1.73, −12.69, −186.58); Card slot 1335 at (−3.57, −10.89, −184.80) | Opens Security Station door 2080; useful loot/medical access, not a plot gate |

Contained cards and logs must be looted via the corpse/container MFD. Finding
their editor coordinates and calling a debug give/teleport is not genuine play.

## Critical path

### Phase A — establish the coolant-radiation problem (eng1)

1. Arrive from the MedSci maintenance conduit around
   (44.6, −15.2, −40.4). Polito's first-visit objective is to reset the Engine
   Core and restore elevator power.
2. Follow the coolant-tube route to the Engine Core access computer / sealed
   door. `Core Access` entity 459 is at (−1.69, −14.0, −29.60). The protective
   seals should prevent Engine Core access while radiation remains; the player
   is directed to purge the tubes.
3. Reach Engineering Control. Loot Sanger's log 13 (entity 451) by its locked
   entrance and open its media panel. Read the in-game transcript that directs
   the player to Cargo Bay 2 for the changed code; record `Note_1_6` becoming
   active before leaving.
4. Travel normally to the south bulkhead and frob eng1
   `Bulk_On_Button` 1124 to enter eng2.

Merely knowing 15061 out of band is insufficient coverage. A focused regression
may enter a known code, but an end-to-end campaign session should acquire the
lead and Sanger log through play.

### Phase B — Cargo Bays: obtain the access card and Sanger's code (eng2)

5. In Cargo Bay 1A, frob corpse 233 and click the **Cargo Bay 2A/2B access
   card** (entity 520) in its container UI.
6. Use that card on the Cargo Bay 2 reader. The reader is Card slot 1585 at
   (18.66, −10.97, −251.56), switching Cargo Bay doors 1378 and 1391.
7. Traverse Cargo Bay 2 to its upper level (cargo lifts are part of the genuine
   route). In Cargo Bay 2B, frob corpse 507 and loot Sanger's **“Locked in”**
   audio log 1426. Open its media panel and visibly read **15061** before using
   that code; record the `Note_1_6` → `Note_1_8` objective progression.
8. Return through eng2 `Bulk_On_Button` 1080 to eng1. Inventory and quest state
   must survive this round trip.

### Phase C — open Engineering Control and prepare the backdoor (eng1 → eng2)

9. Enter **15061** on Engineering Control keypad 1181 and go inside.
10. Attempt/use the Fluidics controls and acquire Delacroix's “Fluidics
    backdoor” log 1421. Open its media panel and visibly read the 45m/dEx
    instructions and **34760** before going to Auxiliary Storage 5. Record the
    `Note_1_13` override objective and its transition to the acquire/install
    objectives. Before installation, Fluidics must not falsely complete the
    purge: its invisible frob target (`Fluidic Button` 1114 at
    (−0.003, −0.624, −177.207)) is filtered on `CircuitBoardPlaced`.
11. Travel through the irradiated coolant area to Auxiliary Storage 5. Use
    **34760** on keypad 1726 and pick up Part **45m/dEX** (entity 705).
    Radiation is a real environmental constraint; use available rad hypos or
    the optional hazard suit rather than raw teleporting through the tubes.
12. Carry 45m/dEX back through the bulkhead to eng2, reach upper Command
    Control, and use it on the **Systems Monitoring Unit / Card Box 676**. The
    item should be consumed and `CircuitBoardPlaced` should change state.
13. Return through the bulkhead to eng1. This is the second required
    cross-mission persistence check.

### Phase D — purge the tubes and enter the Engine Core (eng1)

14. Use `Fluidic Button` 1114 in Engineering Control again. With
    `CircuitBoardPlaced` set, QB Filter 523 fires Once Router 287. The intended
    consequences are observable and stateful:

    - the Fluidics computer changes from off to on;
    - radiation in the coolant tubes is purged;
    - `EngineRadClear` is set;
    - objectives `Note_1_7` and `Note_1_2` complete;
    - Core Access changes/unlocks so the Engine Core can be entered.

15. Walk back through the now-purged coolant route and enter the Engine Core.
    Passing a still-closed access door by collision exploit or teleport is not
    completion.

### Phase E — restart both nacelles and master power (eng1)

16. In the port nacelle control room, frob the **Port Nacelle Computer** 1736
    at (32.15, −13.86, 19.24). It sets `Note_1_3` complete.
17. In the starboard nacelle control room, frob the **Starboard Nacelle
    Computer** 1834 at (−41.90, −13.87, 19.24). It sets `Note_1_4` complete.
    Order does not matter, but both are required.
18. Both computers feed Multi Trigger 881. Only after both have fired should
    `NacellesFrobbed` be set and the Master Power objective become available.
19. Ride the Engine Core grav lift to the upper master-control room. Use the
    visible **Master Power Computer** 1103 / overlaid frob target
    `Master Power Button` 1870 at (1.376, 5.936, −16.688).
20. Its QB filter/router must perform the real completion effects:

    - master-power computer changes from off to on;
    - `CorePower` is set;
    - `Note_1_5` and `Note_1_1` complete;
    - `ElevState` changes;
    - Unlock Trap 1213 unlocks elevator-door control 990;
    - Polito sends the “Core online” message directing the player to the
      elevator.

### Phase F — unlock Hydroponics

21. Return to the main elevator near z≈−111. The original seed mistook this for
    a local vertical connector; it is the inter-deck elevator.
22. Frob `Master Elevator Button` 629, then click **Hydroponics (3)** in the
    elevator MFD. A genuine transition lands in **`hydro2.mis` loc 22**.
23. Preserve the frontier only after confirming the Hydro mission loaded with
    inventory and Engineering quest state intact.

## Ordered objective evidence

Engineering's objective list is a state machine, not a bag of final bits.
Capture `/v1/quests` and the corresponding world/media evidence at each stage
below. An objective that never becomes active, completes out of order, or is
skipped because the player already knows a code is a progression failure.

| Order | Required authored progression |
| --- | --- |
| 1 | Read Sanger log 13 in-game → `Note_1_6` (find the Engineering Control code) becomes active |
| 2 | Loot/read Sanger log 5 from corpse 507 → `Note_1_8` conveys 15061; the earlier search objective advances |
| 3 | Enter Engineering Control and encounter the blocked Fluidics path → `Note_1_13` (find an override) is represented |
| 4 | Read Delacroix log 7 in-game, enter 34760, and take 45m/dEx → `Note_1_10` advances/completes |
| 5 | Insert 45m/dEx in Command Control → `Note_1_9` completes |
| 6 | Use Fluidics after installation → `Note_1_7` and `Note_1_2` complete |
| 7 | Reset starboard and port nacelles → `Note_1_4` and `Note_1_3` complete |
| 8 | Reset Master Power → `Note_1_5` and `Note_1_1` complete |

`Note_1_11` (Utility Storage 4) remains optional. `Note_1_14` is the next-deck
directive and need not complete inside Engineering.

## Genuine-play acceptance gate

Mark Engineering **PASS** only if one reviewed session (or a frontier-backed
sequence of sessions) demonstrates all of the following:

1. Normal eng1 arrival and physical traversal to the sealed Engine Core /
   Engineering Control; no setup teleport beyond an explicitly recorded
   frontier resume.
2. Sanger log 13 is acquired at the Engineering Control entrance and its
   in-game transcript is visibly read before the Cargo Bay trip; `Note_1_6`
   appears through the authored interaction.
3. Genuine eng1→eng2→eng1 travel through bulkhead buttons 1124/1080.
4. Cargo Bay 2 card is looted from corpse 233 and actually used to open Cargo
   Bay 2; Sanger log 5 is looted from corpse 507 and its transcript visibly
   conveys 15061 before the keypad is used; `Note_1_8` advances.
5. Keypad 1181 accepts the learned 15061 and the player enters Engineering
   Control. The pre-install Fluidics attempt must not purge the tubes.
6. Delacroix log 7 is acquired and its in-game transcript visibly conveys the
   45m/dEx route and 34760 before Auxiliary Storage 5 is opened. Evidence shows
   the `Note_1_13` → `Note_1_10` / `Note_1_9` progression.
7. 45m/dEx is obtained behind keypad 1726 (34760), carried across a mission
   transition, inserted into Card Box 676, consumed, and
   `CircuitBoardPlaced` persists after returning to eng1; `Note_1_10` and
   `Note_1_9` advance at their corresponding interactions.
8. Fluidics activation happens only after the override is installed and
   produces the purge/Core Access effects. Evidence should include quests
   `Note_1_7` + `Note_1_2`, `EngineRadClear`, a changed Fluidics/Core Access
   state, and normal passage into the Core.
9. Both distinct nacelle computers are frobbed in-world; `Note_1_3` and
   `Note_1_4` complete and `NacellesFrobbed` changes.
10. Master Power is then activated in-world; `CorePower`, `Note_1_5`, and
   `Note_1_1` reflect completion and elevator control 990 is unlocked.
11. The ordered quest evidence includes `Note_1_6`, `_8`, `_13`, `_10`, `_9`,
    `_7`, `_2`, `_4`, `_3`, `_5`, and `_1`; no final-state-only snapshot may
    substitute for the interaction-by-interaction record.
12. The player returns normally to the main elevator, selects Hydroponics in its
   actual MFD, and loads hydro2 with carried inventory/quest state preserved.
13. Screenshots and `data.json` steps show each major interaction and its
    consequence. Quest-state assertions alone are not proof if the player
    debug-messaged traps or skipped world traversal.

Any session that only reaches eng2, directly enters known codes without the
campaign's discovery path, presses an elevator that happened to be available,
or forces a transition to Hydro is **invalid/shallow**, not a clean pass.

## Engine watch-points / likely feature-gap boundaries

- The port currently treats an unset `ElevState` as elevator-accessible for
  playability (`scripts/gui/elevator.rs`). Therefore **successful elevator
  travel is not proof that power was restored**; the quest/object evidence in
  the acceptance gate is mandatory.
- **Log quest-bit metadata is a current objective blocker.** Sanger log 13
  (entity 451) carries `PropQuestBitName("Note_1_6")`, and Delacroix log 7
  (entity 1421) carries `PropQuestBitName("Note_1_9")`; neither has a
  SwitchLink fallback. The current `MediaGui` / `CollectLog` path records and
  displays a log but does not apply that metadata. If reading either log fails
  to advance its authored objective sequence, mark Engineering blocked and fix
  the log interaction. Do not inject the bit or accept a readable transcript
  as a substitute for objective progression.
- Card/corpse container looting, keypad entry, card readers, cross-level
  inventory/quest persistence, cargo/grav lifts, `ObjConsumeButton`,
  `TrapQBFilter`, `TriggerMulti`, model swaps, radiation clearing, and elevator
  MFD interaction are all independently exercised by this path. Report the
  first failed real mechanism as the blocker; do not replace it with a debug
  message or transition.
- Radiation survival equipment is optional in the original path. Missing the
  optional Security Station or hazard suit is not itself a failed playtest;
  failure of the purge to remove the hazard or open Core Access is.
- The nine “RadKey Card”/radbutton observations in the old seed were a
  misidentification of the Auxiliary Storage 5 area. The plot item there is the
  ordinary Circuitboard entity 705, displayed as **Part #45M/dEX**.
- No NPC named Delacroix is expected on this deck. Her audio log supplies the
  backdoor instructions.

## Sources

- Retail data: `Data/res/strings/NOTES.STR` (ordered Engineering objectives),
  `Data/res/strings/LEVEL01.STR` (Polito emails and Sanger/Delacroix logs), and
  `OBJNAME.STR` (Part #45M/dEX / computer display names).
- Mission wiring: `cargo dq entities eng1.mis ...` and
  `cargo dq entities eng2.mis ...` (2026-07-23), especially entities 459, 629,
  705, 1114, 1124, 1181, 1726, 1736, 1834, 1870 (eng1) and 233, 507, 520, 676,
  1080, 1426, 1585 (eng2), plus their SwitchLinks and quest-bit traps.
- [SystemShock.org objective list](https://www.systemshock.org/index.php?topic=48.0)
  — ordered Engineering objectives and codes.
- [System Shock Wiki — Engineering Deck](https://shodan.fandom.com/wiki/Engineering_Deck)
  — first-visit synopsis, optional Security card, and A/B deck split.
- [GameFAQs walkthrough (Bahamut_Zero)](https://gamefaqs.gamespot.com/switch/513867-system-shock-2-25th-anniversary-remaster/faqs/8959)
  — coolant tubes, Cargo Bays, Engine Core, and elevator progression.
- [RPGClassics walkthrough](https://archive.rpgclassics.com/shrines/pc/sysshock2/walkthrough.shtml)
  — both nacelles, master-power room, and main-elevator handoff.
