# Walkthrough — Hydroponics (hydro2 → hydro1 / hydro3 → hydro2 → Operations)

Context for the `playtest` agent and the `play-through` reviewer. This is the
**real Deck 3 objective path**, cross-checked against retail walkthroughs and
the entities, links, and quest-bit traps in `hydro1.mis`, `hydro2.mis`, and
`hydro3.mis`. It is not a movement script: observe and react, but do not count
touching the three maps or taking the elevator as completion.

Hydroponics' plot objective is to restore the main elevator's Hydro display.
The player must trigger the authored research assignment, obtain and genuinely
research Toxin-A, collect enough vials, insert one into each of four
Environmental Regulators, and return to the main elevator before going to
Operations.

Positions below are mission `PropPosition` coordinates and are useful search
anchors. Mission object ids are stable only within the named `.mis` file;
runtime entity ids are not stable. At runtime, resolve by name and `template_id`.

## Valid campaign start

The campaign reaches **`hydro2.mis`**, the central Hydroponics B/C map, through
the main elevator after restoring it in Engineering. Preserve the carried
inventory, keycards, player upgrades, and quest bits from that transition.

A fresh launch directly into `hydro2.mis` is useful for investigation but is
**not** an end-to-end acceptance start. In particular, the port deliberately
treats an unset `ElevState` as an accessible elevator, so a fresh-launch tester
can click Operations without exercising Hydroponics. Likewise, manually setting
quest bits, giving plot items, teleporting through doors, or invoking a direct
level transition invalidates objective coverage.

## Deck topology and legitimate transitions

| from | world interaction | mission object / position | destination |
| --- | --- | --- | --- |
| Engineering / another tram stop | Main Elevator panel | `hydro2` Master Elevator Button id 537, tmpl −1723 @ (7.2, −0.4, 0.0) | `hydro2`, elevator marker 22 |
| `hydro2` (B/C) | Sector A bulkhead button | id 998, tmpl −3622 @ (39.4, 3.6, −70.0) | `hydro1`, loc 301 |
| `hydro1` (A) | return bulkhead button | id 442, tmpl −3622 @ (7.8, 2.0, 50.4) | `hydro2`, loc 301 |
| `hydro2` (B/C) | Sector D bulkhead button | id 1617, tmpl −3622 @ (60.0, 2.8, 69.8) | `hydro3`, loc 302 |
| `hydro3` (D) | return bulkhead button | id 408, tmpl −3622 @ (28.0, 2.8, −25.4) | `hydro2`, loc 302 |
| `hydro2` | Main Elevator panel, click **Operations (4)** | Master Elevator Button id 537 | **`ops2.mis`**, elevator marker 22 |

`hydro2` is the hub and contains the B and C regulators. `hydro1` is Sector A;
`hydro3` is Sector D. All three missions contain copies of the quest-bit
completion check, so the fourth regulator can legitimately be the last one
visited in any order.

## Plot items and objective anchors

### Toxin-A

Toxin-A is template **−1341**, `Anti-Annelid Toxin`. There are five vials in
the shipped missions; four are consumed by the four regulators:

| mission object | legitimate source |
| --- | --- |
| `hydro2` id 2140 @ (12.8, 0.3, −53.2) | loose vial |
| `hydro2` id 547 | in Desk id 713 @ (25.8, −1.3, 42.4) |
| `hydro2` id 1185 | in Desk id 675 @ (25.8, −1.3, 35.6) |
| `hydro2` id 1186 | in chemical-storage Desk id 354 @ (60.5, −2.5, −40.0); the desk also holds Vanadium and two Antimony samples |
| `hydro1` id 1199 | spare vial in MS Female Corpse id 466 @ (17.2, 0.0, 56.2) |

The loose vial, `hydro2` id 2140, is the most visible introduction to the item,
but **picking it up does not initiate the objective**. Three
`TrapNewTripwire` volumes independently route to `EVENT1-Router` id 1437:

| tripwire | position |
| --- | --- |
| `hydro2` id 1323 | (21.6, −0.4, 39.2) |
| `hydro2` id 1322 | (14.2, −0.4, −58.8), near loose vial 2140 |
| `hydro2` id 2087 | (57.6, −1.6, −41.8) |

Crossing any one through normal movement immediately drives
`NOTE_3_2=INCOMPLETE` and email 302. Its delayed branch awards 10 cyber modules
after 20 seconds / 1200 fixed frames. A depth pass must record the real Event-1
tripwire consequence and separately pick up vial 2140 in-world; do not infer
either one from the other or reject an otherwise valid order merely because
the tripwire fired first.

The two desks around x≈25 are the toxin laboratory. Retail walkthroughs
describe a jammed-door / broken-window approach, but local mission inspection
does not expose a breakable lab-window entity. Use the observed door/aperture
route rather than assuming a particular pane: stable anchors are HydroDoorWide
id 183 @ (23.8, −0.4, 46.0), Science Table id 716 @
(19.2, −1.1, 38.4), desks 675 / 713, and Event-1 tripwire 1323.
Container contents must be taken through the real loot panel; they are not
floor objects.

Original behavior requires **Research 1**, supplied either by buying the skill
with the Event-1 module award or using the nearby **LabAssistant** implant
(`hydro2` id 1064, tmpl −969 @ (25.6, −0.6, 35.9)). Research consumes Antimony
(Sb), Vanadium (V), then Antimony and completes quest bit `Note_3_2`. The
chemical-storage desk above contains the required V and two Sb samples.

### Access cards

| card | source | real gate |
| --- | --- | --- |
| Hydro B, tmpl **−1495** | `hydro2` id 942, inside HYD Male Corpse id 754 @ (72.1, −1.7, −15.6), by the monkey corridor | B card readers, region 128 (for example ids 1120 / 1122) |
| Hydro A, tmpl **−1494** | `hydro2` id 934, inside HYD Female Corpse id 1297 @ (46.1, −8.6, −39.4), in cold-storage / B maintenance | A card readers, region 64 (ids 1132 / 1597), then the `hydro1` bulkhead |
| Hydro D, tmpl **−1496** | `hydro1` id 79, inside HYD Male Corpse id 471 @ (38.1, 0.0, 9.1), in the first Sector A cultivation area | D card readers, region 256 (ids 1152 / 1177), then the `hydro3` bulkhead |

Loot each corpse and click the card in the container MFD. Do not use
`/v1/player/give` for contained cards.

### Environmental Regulators

Every regulator accepts only an entity whose `PropSymName` is
`Anti-Annelid Toxin`. In flat mode, frobbing the regulator consumes the first
matching carried vial. In VR, provide the held vial for consumption.

| regulator | area and route anchor |
| --- | --- |
| **ACR1** | `hydro1` id 141, tmpl −2659 @ **(28.8, 2.4, −68.2)** — far end of Sector A cultivation |
| **ACR2** | `hydro2` id 913, tmpl −1151 @ **(33.4, −6.0, −50.0)** — icy Sector B maintenance / cold storage; descend through the floor opening and use the ladder stack around (33.7, −4.8, −53.6) to leave |
| **ACR3** | `hydro2` id 910, tmpl −1151 @ **(76.0, −2.0, 23.4)** — Sector C maintenance; climb the ladder around (59.6, −2.0, 22.2), pass through the office, and take the downward ramp |
| **ACR4** | `hydro3` id 247, tmpl −1151 @ **(43.6, −1.6, −40.2)** — Sector D, down the ramp past the control / turbine area |

Concrete traversal anchors for the two central routes:

- **Sector C / ACR3:** Rick Ladder ids 551–564 form the vertical stack at
  x=59.6, z=22.2 from y=−2.0 to 6.0; breakable CrackedGlass id 657 is at
  (57.0, 2.6, 18.4); upper doors 1196 / 1195 and tripwire 1194 are around
  (62.2, 4.0, 27.2); corpse 406 and ACR3 are at the lower far end. The
  descending ramp itself is brush geometry and has no entity id.
- **Cold storage / ACR2:** FrostedGlass1 id 795 @ (28.4, 3.6, −46.4) is the
  concrete breach/drop anchor; ladder ids 110 / 520 are at
  (33.65, −4.8, −53.6) and (33.65, −0.8, −53.6); ACR2 and the Hydro A-card
  corpse 1297 are on the lower route. The floor opening is brush geometry and
  has no entity id.

## Critical path

### 1. Establish the real objective in central Hydro B/C (`hydro2`)

1. Arrive through the main elevator with the Engineering campaign state.
2. Explore the security / Biological Survey side instead of selecting another
   elevator floor. Deal with hybrids, monkeys, Midwives, eggs, cameras, and
   other threats using normal player combat or avoidance.
3. Cross one of the real Event-1 tripwires through bounded movement. Confirm
   email 302 and `Note_3_2=INCOMPLETE` immediately, then wait 1200 fixed frames
   and confirm the delayed 10-module award. Separately reach loose Toxin-A vial
   id 2140 at (12.8, 0.3, −53.2) and pick it up in-world.
4. Reach the Toxin-A laboratory through an observed, collision-respecting
   door/aperture route, using door 183, table 716, and desks 675 / 713 as
   evidence anchors. Loot the LabAssistant implant if needed, additional vials,
   and the required chemicals. Do not manufacture a breakable-window test:
   local data contains no substantiated breakable lab pane.
5. **Research Toxin-A through the real research flow.** Confirm `Note_3_2` is
   complete before treating any vial as usable. Merely possessing the
   `Anti-Annelid Toxin` entity is not research coverage.

### 2. Exercise both central-map regulators and their traversal gates

6. Reach Sector C maintenance by genuinely climbing the ladder, crossing the
   office, and descending the ramp. Insert a researched vial into **ACR3**.
7. Reach the B monkey corridor and loot the **Hydro B card** from its corpse.
   Use the actual B card reader rather than moving through the locked door.
8. Enter the cold-storage drop, reach **ACR2**, insert a second researched vial,
   and loot the **Hydro A card** from the nearby corpse. Climb back out rather
   than teleporting to the upper floor.
9. Obtain the remaining vials from the lab / chemical storeroom as needed.

The order of ACR2 and ACR3 may vary. What matters is that both the ladder/ramp
route and the cold-storage drop/return are honestly traversed.

### 3. Complete Sector A (`hydro1`)

10. Use the Hydro A card at its reader, walk to the Sector A bulkhead, frob its
   real button, and transition to `hydro1`.
11. Traverse the cultivation rooms. Loot the **Hydro D card** from the Sector A
    corpse; the nearby corpse with the spare Toxin-A may be used if only three
    central-map vials were collected.
12. Reach **ACR1** at the far end and insert a researched vial.
13. Walk back and use the return bulkhead button to reach `hydro2`.

### 4. Complete Sector D (`hydro3`)

14. Use the Hydro D card at its reader, reach the Sector D bulkhead, and take
    the real transition to `hydro3`.
15. Traverse the grim entrance, control / turbine rooms, ramp, enemies, and
    eggs to **ACR4**. Insert one researched vial.
16. Require ACR4's previously unknown quest bit to change to
    `INCOMPLETE` / raw 1, its regulator model to change, and inventory to fall
    by exactly one vial; then return through the bulkhead to `hydro2`.

Sector A and Sector D can be completed in either order after their cards are
available. Do not infer success merely from having visited both missions.
After whichever unique regulator is actually fourth—ACR4 when A precedes D,
or ACR1 when D precedes A—allow at least 10 seconds / 600 fixed frames for the
shipped 1-, 3-, and 6-second completion delays and objective messages.

### 5. Verify completion and take the real exit

17. Inspect `/v1/quests`. Each regulator activation bit **`ACR1`…`ACR4` must
    have changed from unknown to `incomplete` (raw bits 1) at its own unique
    insertion**. This counterintuitive value is the mission's “activated”
    marker, not evidence that insertion failed.
18. Genuine completion of all four drives the linked checks and delays:
    `Note_3_1=complete`, `Note_3_3=complete`, `ElevState=complete`, and the next
    objective `Note_3_4=incomplete`. `Note_3_2=complete` independently proves
    that Toxin-A was researched.
19. Return to the Hydro main-elevator lobby and reproduce a screenshot from the
    same viewpoint used on arrival. The broken `B_Hydro_Screen` / `elsha`
    display must be gone and the working `Hydro_Screen` / `elhyd` display must
    be present. The completion chain destroys object 550 and teleports object
    710 into place; quest bits alone do not prove this world consequence.
20. Frob Master Elevator Button 537, select the labeled
    **Operations (4)** button in the real elevator MFD, and verify the mission
    becomes **`ops2.mis`**. Save the campaign frontier there.

## Acceptance checklist — what counts as genuine play

A Hydroponics pass is valid only if the session evidence shows all of:

- Entry from the advancing Engineering campaign state, with no fresh-mission
  quest reset or direct Hydro launch used as proof.
- One Event-1 tripwire crossed through bounded movement; email 302 and
  `Note_3_2=INCOMPLETE` recorded immediately and the delayed 10-module award
  recorded after 1200 frames. Loose Toxin-A vial 2140 is also reached and
  picked up in-world as separate evidence.
- Research 1 or the LabAssistant obtained legitimately, Sb/V/Sb supplied, and
  `Note_3_2=complete` through the real research mechanic.
- Four distinct vials consumed by four unique machines: before each use its
  matching ACR bit is unknown, after use that bit is raw 1, its model changes,
  and inventory count falls. Re-frobbing one already activated regulator does
  not count as another insertion.
- After the fourth unique insertion and its delayed completion evidence, save
  the game, record the fifth vial's entity/count, and re-frob one specifically
  identified activated regulator. A correct negative probe leaves inventory
  and objective/world effects unchanged. If the vial is consumed, file the
  gameplay bug and reload the pre-probe save before advancing.
- Hydro B, A, and D cards looted from their real corpses and their actual card
  readers used.
- Genuine traversal of the lab's observed door/aperture route, the Sector C
  ladder/ramp, the cold-storage breach/drop and ladder return, and the
  `hydro1` / `hydro3` bulkheads using bounded movement. Evidence uses the stable
  anchors listed above on both sides of each brush-geometry passage. A raw
  teleport is allowed only to resume a saved frontier, never to bridge one of
  these gates.
- Threats encountered on the critical route handled with normal combat or
  sensible avoidance—not debug `Damage` messages as a substitute for play.
- Final quest state recorded after the delays: `ACR1..4` raw 1,
  `Note_3_1`, `Note_3_2`, `Note_3_3`, and `ElevState` complete.
- Before/after screenshots from the same elevator-lobby viewpoint show the
  broken `elsha` display replaced by the working `elhyd` Hydro display.
- Physical return to the `hydro2` elevator and a real Elevator MFD transition
  to `ops2`.

The following are specifically **insufficient**: visiting all three mission
files, frobbing only their return buttons, consuming unresearched vials because
the port permits it, setting ACR / Note / ElevState bits through the debug API,
starting with a desk vial while skipping both the Event-1 tripwire and loose
vial pickup, citing `ElevState` or an enabled Operations button without the
authored display repair, repeatedly feeding vials to one regulator, clicking
Operations before completing the regulators, or direct-transitioning to
`ops2`.

## Engine watch-points for triage

1. **Research is a likely feature-gap blocker.** As of 2026-07-23,
   `ResearchableScript`, `ResearchableUseScript`, and the LabAssistant's
   `StatBoostImplant` are registered to `UnimplementedScript` in
   `shock2vr/src/scripts/mod.rs`; Toxin-A's research-time, chemical, and text
   metadata are also unparsed. Meanwhile `ObjConsumeButton` checks only the
   carried item's symbolic name, so the port may consume an unresearched vial.
   If a real playtest cannot perform research, classify it as a `[gameplay]`
   **feature gap**; neither buying Research 1 nor merely equipping LabAssistant
   proves the missing research flow, and the consumption bypass must not be
   blessed.
2. **Cross-mission state is essential.** Cards, remaining vials, `ACR1..4`, and
   research/objective bits must survive both `hydro2↔hydro1` and
   `hydro2↔hydro3`, plus save/reload at the frontier.
3. **Completion is delayed.** The mission chains 1-, 3-, and 6-second trap
   delays; step at least 600 frames after the fourth regulator before reporting
   a missing state transition.
4. **`ElevState` and Operations access are not Hydro completion evidence.**
   Current elevator gating allows Operations both when `ElevState` is unset
   (raw 0) and when it is `INCOMPLETE` (raw 1), including an honest
   Engineering-derived campaign. Require `ACR1..4`, the `Note_3_*` sequence,
   and a same-view before/after showing `elsha` replaced by `elhyd`. Reaching
   `ops2` proves only the exit interaction.
5. **Traversal is coverage.** Ladder climbing, observed lab access, the
   cold-storage breach/drop, corpse/desk container UI, keycard readers,
   regulator item consumption, and the elevator MFD are all required
   mechanics. Report a blocker at the first failed real gate rather than
   warping past it.
6. **Repeated regulator use is unsafe in the current port.** `ObjConsumeButton`
   has no activated-state guard and can destroy another matching vial on a
   second Frob even though the one-shot ACR/XP traps have already disappeared.
   Identify the unique regulator and verify its bit/model transition before
   counting an insertion. Preserve the fifth vial for a save/reload negative
   probe: if an already activated regulator consumes it, file a gameplay bug
   and resume from the pre-probe save rather than corrupting the advancing
   inventory.

## Sources

- [SShock2.com full walkthrough, Deck 3](https://www.sshock2.com/ss2walk/) —
  detailed B/C → A → D route, cards, regulator locations, and return to the
  elevator.
- [GameFAQs guide by garkimasera, Deck 3](https://gamefaqs.gamespot.com/pc/185706-system-shock-2/faqs/27508) —
  concise critical path, research chemicals, four regulators, and module
  rewards.
- [GameFAQs guide by DC, Deck 3](https://gamefaqs.gamespot.com/pc/185706-system-shock-2/faqs/7173) —
  independent route description for the toxin lab, card chain, cold storage,
  Sector A, and Sector D. Its window wording is treated as narrative guidance,
  not as a local entity assertion.
- [Port Forward Hydroponics walkthrough](https://portforward.com/games/walkthroughs/System-Shock-2/Hydroponics.htm) —
  screenshot-backed lab, research, traversal, and regulator sequence.
- Local `cargo dq entities hydro1.mis|hydro2.mis|hydro3.mis` inspection
  (2026-07-23) — item containment, stable templates, positions, key regions,
  transition destinations, regulator SwitchLinks, and quest-bit completion
  chain.
- `shock2vr/src/scripts/obj_consume_button.rs`,
  `shock2vr/src/scripts/mod.rs`, and `shock2vr/src/scripts/gui/elevator.rs` —
  current port behavior and the research / consumption / elevator watch-points.
