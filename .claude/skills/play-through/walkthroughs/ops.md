# Walkthrough — ops1 / ops2 / ops3 / ops4 (Operations, deck 4: the SHODAN reveal + the three Sim Units)

Context for the `playtest` agent and the `play-through` reviewer. This is the
**story-critical Operations path**, cross-checked against public retail
walkthroughs and then verified against `ops1.mis` … `ops4.mis` entity data with
`cargo dq`. Reaching a bulkhead or riding an elevator is *not* completion:
Operations is complete only after the player has taken the SHODAN reveal in
Polito's office and reprogrammed all three Simulation Units with their three
override chips, through the real in-world objects.

This is not a movement script. Observe and react, traverse with bounded
movement, fight or evade, and use the anchors below to identify the intended
objective rather than teleporting between coordinates.

**Positions and positive IDs below are mission-file object identities** from
`cargo dq`. Runtime entity IDs are assigned per launch and are **not stable** —
resolve entities anew each run by name and by the stable `template_id`, and
never hardcode a runtime entity ID.

## What the four Ops missions actually are

Operations is one deck split into four map sections. The shipped mission files
map 1:1 onto the sectors the public walkthroughs call Ops A–D:

| File | Sector | What it is | Size |
| --- | --- | --- | --- |
| `ops2.mis` | **Ops B** | **The hub and the deck's real entry point.** Main lobby off the main elevator, XERXES core/crates, bio-reconstruction, Systems Administration + conference room, Crew Quarters blocks, Chemical Store Room, upgrade lounge. All three outbound bulkheads live here. | 1005 objects |
| `ops1.mis` | **Ops A** | **Dr. Polito's office — the SHODAN reveal.** A tiny, one-way, cutscene-only leaf: SHODAN screen wall, force-door FX, egg/grub cutscene props, four "Rumbler" set pieces, one corridor. | 225 objects |
| `ops3.mis` | **Ops C** | Data Storage, lounges/upgrade rooms, Mess Hall + galley/cold storage, radiation-leak corridor, Operations Offices — **Sim Unit 1 (Quantum)**, both Crystal Shards, Chip C's carrier. | 863 objects |
| `ops4.mis` | **Ops D** | Power Ops / Power Administration, System Operations, Fluid Ops, Barracks + firing range + Brig, the armory keypad, Security stations, Command Center / Ops Override — **Sim Unit 2 (Linear)** and **Sim Unit 3 (Interpolated)**, Chip B's carrier. | 771 objects |

`ops1.mis` being tiny and cutscene-shaped is confirmed by its contents: SHODAN
screen tiles (`SHODAN_Screen_TL/TM/TR/BL/BM/BR`), `CutSceneNine`,
`EggsandGrubsControl` + `FirstEgg` (`CS9_EggsandGrubs` / `GrubEgg` scripts),
`MasterForceField` routing five `force door fx`, `Rumbler1..4` with
`RumblerLoc1..4`, `Polito_Sign` ×2, and a single `Bulk_On_Button`.

Note for map cross-reference: sshock2.com ships `map_ops2/3/4.html` but
`map_ops1.html` is a "no map available" placeholder — the retail game has no
automap for sector A either. The walkthroughs' "Operations deck" prose therefore
maps onto four `.mis` files, not one.

## Arrival, the ops2 hub, and the connector graph

Operations is entered from the **main inter-deck elevator**, which is the only
inter-deck bridge in this port. `shock2vr/src/scripts/gui/elevator.rs`'s
`ELEVATOR_STOPS` list is `["eng1.mis", "medsci1.mis", "hydro2.mis",
"ops2.mis", "rec1.mis"]`, so deck 4 "Operations" loads **`ops2.mis`**, and the
prior campaign observed it arriving at **loc 22**.

`ops2` has four `Level Start Marker`s for the bulkhead locs plus a cluster of
four loc-22 markers (ids 560, 1019, 1020, 1021, 1022, all `PropStartLoc(22)`
around (17.2…20.0, −8.1…−8.2, −1.4…−4.0)) — the elevator car floor.

| Purpose | Stable mission object | Position | Destination |
| --- | --- | --- | --- |
| **Main elevator panel (ops2 hub)** | `Master Elevator Button` **537** (`S$ElevatorButton`) | (19.99, −8.10, −1.41) | Opens the five-deck elevator MFD; arrival loc for the deck is **22** |
| ops2 → **ops1** (Polito's office) | `Bulk_On_Button` **361** → hidden changer **1001** | (16.60, −7.90, 4.30) | `ops1`, loc **100** |
| ops1 → ops2 | `Bulk_On_Button` **285** | (19.40, −7.90, 8.50) | `ops2`, loc **100** |
| ops2 → **ops3** | `Bulk_On_Button` **185** → gated changer **992** | (31.50, −7.90, 17.80) | `ops3`, loc **200** |
| ops3 → ops2 | `Bulk_On_Button` **742** | (27.20, −7.90, 25.80) | `ops2`, loc **200** |
| ops2 → **ops4** | `Bulk_On_Button` **404** → gated changer **999** | (52.90, −7.90, −8.20) | `ops4`, loc **300** |
| ops4 → ops2 | `Bulk_On_Button` **556** | (49.60, −7.90, −20.50) | `ops2`, loc **300** |
| ops2 → **ops3 (second, far bulkhead)** | `Bulk_On_Button` **409** | (59.00, −7.90, 142.70) | `ops3`, loc **400** |
| ops3 → ops2 (far) | `Bulk_On_Button` **927** | (63.40, −7.90, 146.90) | `ops2`, loc **400** |

Matching arrival markers: ops1 loc 100 = (16.60, −7.90, 4.30) [object 72];
ops2 loc 100 = (19.40, −7.90, 8.50) [365], loc 200 = (35.70, −7.90, 15.00)
[420], loc 300 = (47.80, −7.90, −6.90) [433], loc 400 = (61.80, −7.90, 146.90)
[443]; ops3 loc 200 = (23.00, −7.90, 28.60) [921], loc 400 = (60.60, −7.90,
142.70) [735]; ops4 loc 300 = (54.70, −7.90, −21.80) [340].

**ops1 is a leaf hanging off ops2, and ops3/ops4 are gated.** This is the single
most important structural fact on this deck, and it is authored in the data, not
inferred:

- The three ops2 buttons the player actually touches (185, 361, 404) carry a
  **mission-level `PropScripts { scripts: ["BaseButton"], inherits: false }`**
  that *overrides* the `Bulk_On_Button` template's `LevelChangeButton`. They do
  not change level themselves. Co-located with each is a hidden second object
  (992, 1001, 999) that keeps `LevelChangeButton` and carries the real
  `PropDestLevel` / `PropDestLoc`.
- Button **361** (→ ops1) SwitchLinks **straight** to changer 1001. It works
  from the moment the player arrives.
- Button **185** (→ ops3) links to `QB Filter` **995** and `Anti-QB Trigger`
  **993**; button **404** (→ ops4) links to `QB Filter` **998** and `Anti-QB
  Trigger` **997**. Both filters are keyed on the quest bit **`ShodanRoom`**,
  and only then fire changers 992 / 999. The Anti-QB triggers have **no
  outgoing links at all** — before `ShodanRoom` is set, pressing those bulkhead
  buttons does *nothing*.
- Conversely, `Simple QB Trigger` **1003** (`ShodanRoom`, at (12.94, −7.90,
  4.23)) fires **`KillBulk1` 1002**, a `TrapDestroy` aimed at changer **1001**.
  Once the reveal has happened, **the way back into Polito's office is
  destroyed**. Sector A is one-way and one-time.

So the visit order is forced: **elevator → ops2 hub → ops1 (reveal) → back to
ops2 → then ops3 and ops4 in either order.**

## Ops A (`ops1`) — the SHODAN reveal, and everything it unlocks

Route: arrive at (16.60, −7.90, 4.30) and walk in −x along the corridor.

| Object | Position | What it does |
| --- | --- | --- |
| `New Tripwire` **80** | (1.20, −8.80, 6.40) | fires `Ops Crew` 79 (scripted crew set piece) |
| `New Tripwire` **277** | (−9.16, −7.40, 6.40) | → `QB Filter` **278** (`ShodanRoom`) → `EmailTrap` **279** |
| **`New Tripwire` 204 — the reveal trigger** | (−13.73, −7.89, 6.22) | → `CutSceneNine` 135, `Marker` 12, and four QB sets |
| `QB Set` **114** | (−1.76, −8.60, −14.50) | sets **`ShodanRoom` = INCOMPLETE** (i.e. the bit becomes set) |
| `QB Set` **264** | (2.71, −11.25, 11.07) | **`Note_2_1` = COMPLETE** (MedSci's "reach Deck 4") |
| `QB Set` **296** | (2.71, −11.25, 13.07) | **`Note_1_14` = COMPLETE** (Engineering's next-deck directive) |
| `QB Set` **297** | (2.71, −11.25, 15.07) | **`Note_3_4` = COMPLETE** ("Get to Deck 4 to meet Dr. Polito") |
| `EmailTrap` **279** | (−4.14, −7.40, 5.55) | `PropLog { deck: 4, email: 1 }` — SHODAN's **"Fix the Sim Units"** — and sets **`Note_4_2` = INCOMPLETE** ("Reprogram the three Sim Units") |

Note the ordering the geometry implies: 277 (x ≈ −9.2) is crossed *before* 204
(x ≈ −13.7) on the way in, so its `ShodanRoom` filter fails and no email fires;
the cutscene at 204 sets the bit; **on the way back out** 277 fires again, this
time passing the filter, and delivers the "Fix the Sim Units" email plus the
`Note_4_2` objective. A session that never walks back out of the office will
never receive the deck's headline objective.

Also present: `MasterForceField` 200 (`TrapRouter`) driving five `force door fx`
at (−2.14, −6.88, 25.21); `EggsandGrubsControl` 145 (`CS9_EggsandGrubs`) at
(−8.12, −7.02, 25.20) driving `FirstEgg` 139 (`GrubEgg`, at (−30.70, −7.10,
41.93)); `SlowDoorControl` 205 / `FastDoorControl` 209 near (−3.5, −6.0,
−16.4) driving the wall/floor/roof "Dummy Door" panels of the reveal set;
`Rumbler1..4` (191, 196, 197, 198) with `RumblerLoc1..4`; `Airlock Door 1/2`
(130, 261, 265, 267); `SafeTeleportLoc` 199.

## The genuine objective chain (ordered, with the in-world gate for each)

Retail objective strings (`Data/res/strings/NOTES.STR`) for deck 4:

- `Note_4_1` — "Find the passcode for the MedSci2 sub armory."
- `Note_4_2` — **"Reprogram the three Sim Units."**
- `Note_4_3` — "Go to the Command Deck."
- `Note_4_4` — "Go to the Recreation Deck"
- `Note_4_5` — "Malick has booby-trapped Sim Unit 3."
- `Note_4_6` — "Weapons lockup code is 13433."
- `Note_4_8` — "Create a data access channel from the Command Center."
- `Note_4_9` — "Go to the Engine Core on the Engineering Deck."

| Order | Where | Required authored progression | Driven by |
| --- | --- | --- | --- |
| 1 | ops2 → ops1 | Enter Polito's office; the reveal cutscene fires; `ShodanRoom` is set and `Note_2_1` / `Note_3_4` / `Note_1_14` complete | tripwire 204 → QB sets 114, 264, 296, 297 |
| 2 | ops1 (on the way out) | SHODAN's "Fix the Sim Units" email arrives and **`Note_4_2` becomes active** | tripwire 277 → filter 278 → `EmailTrap` 279 |
| 3 | ops2 hub | The ops3 and ops4 bulkhead buttons now actually work | filters 995 / 998 gated on `ShodanRoom` |
| 4 | ops2 (Systems Administration) | Kill **`Red Assassin` 254** and loot **Chip A** (Quantum, object 554) from its body | `Contains` link 254 → 554; Chip A also SwitchLinks `Experience Trap` 1089 |
| 5 | ops3 (Data Storage) | Kill/loot **`Docile` 125** for **Chip C** (Interpolated, object 840) | `Contains` link 125 → 840; XP trap 1348 |
| 6 | ops4 (near the Command Center) | Kill **`Red Assassin` 436** and loot **Chip B** (Linear, object 438) | `Contains` link 436 → 438; XP trap 844 |
| 7 | ops3 (Operations Offices) | Use Chip A on **`SimComp_1` 810** → **`Comp1`** set | `ObjConsumeButton`, `PropConsumeType("Chip A")` |
| 8 | ops4 | Use Chip B on **`SimComp_2` 437** → **`Comp2`** set | `PropConsumeType("Chip B")` |
| 9 | ops4 (Power Ops lower) | Use Chip C on **`SimComp_3` 444** → **`Comp3`** set | `PropConsumeType("Chip C")` |
| 10 | whichever unit is last | The three-way QB filter chain passes → **`Reprogram`** set, **`Note_4_2` = COMPLETE**, XP award, and SHODAN's follow-up email | see below |

Each Sim Unit fires the *same* three-stage `Comp1 → Comp2 → Comp3` filter chain
so that the completion happens at whichever unit is used last:

- ops3 `SimComp_1` 810 → `QB Set Comp 1` **1368** (`Comp1`) + `Trigger Delay`
  1369 → `QB Filter Comp 1` **1370** → **1371** (`Comp2`) → **1372** (`Comp3`)
  → `Gone To Rec` 1374 / `Not Gone 2 Rec` 1373 / `QB Set Reprogram` **1376**
  (`Reprogram`) / `Experience Trap` 1375 / `QB Set` **1377** (`Note_4_2` =
  COMPLETE).
- ops4 `SimComp_2` 437 → `QB Set Comp 2` **811** + delay 812 → filters 813
  (`Comp1`) → 814 (`Comp2`) → **815** (`Comp3`) → `QB Gone To Rec A` 816
  (`transmit`) / `Not Gone 2 Rec A` 817 / `QB Set` **483** (`Note_4_2` =
  COMPLETE) / `Reprogram A` 835 / XP 836.
- ops4 `SimComp_3` 444 → `QB Set Comp 3` **448** + delay 449 → filters 450
  (`Comp1`) → 451 (`Comp2`) → **452** (`Comp3`) → `Gone 2 Rec B` 453 / `Not
  Gone To Rec B` 454 / `QB Set` **484** (`Note_4_2` = COMPLETE) / `Reprogram B`
  839 / XP 840.

The `Gone To Rec` / `Not Gone 2 Rec` pair (keyed on the **`transmit`** bit,
which Recreation sets) selects which SHODAN email plays: `Email 9` /
`Email 9A` / `Email 9B` ("I am pleased… proceed to the Recreation deck") if
Rec has *not* been done, or `Email 8` / `Email 8A` / `Email 8B` ("Xerxes is
diminished… I have activated the primary elevator shaft. Take it to deck 6") if
it has. **Ops and Rec are order-independent by design**; a playtest must not
treat "SHODAN said go to Rec" as the only valid outcome.

Each unit also fires an `Anti-QB Trigger` on **`Reprogram`** (ops3 685, ops4
1213 and 1215) → an `Experience Trap`, i.e. the per-unit cyber-module award is
paid only while the deck is not yet finished.

Additional deck-4 side objectives, all data-verified:

| Objective | Activated by | Completed by |
| --- | --- | --- |
| `Note_4_6` "Weapons lockup code is 13433" | ops3 **audio log 1349** (deck 4 / log 7, Bronson "No shirkers") at (18.27, −9.57, 34.02), which carries `PropQuestBitName("Note_4_6")` = INCOMPLETE | ops4 `Keypad` **333** (`PropKeypadCode(13433)`) at (60.59, −8.83, −59.55) → `QB Set` **489** (`Note_4_6` = COMPLETE) + `Ops Door` 331/332 |
| `Note_4_5` "Malick has booby-trapped Sim Unit 3" | ops4 **audio log 465** (deck 4 / log 6, Malick "Sim Unit 3") at (6.32, −9.57, −10.03), `PropQuestBitName("Note_4_5")` = INCOMPLETE | ops4 `New Tripwire` **602** at (−9.89, −15.18, −9.49), immediately at `SimComp_3` (−9.94, −14.80, −7.51). It fires `QB Set` **828** (`Note_4_5` = COMPLETE) *and* opens `Grate` 391 / `Grate` 753 and an **`AI Signal Trap` 612** — that is the spider ambush |
| `Note_4_1` "Find the passcode for the MedSci2 sub armory" | (set on an earlier deck) | ops4 **audio log 525** (deck 4 / log 11, Bronson "MedSci armory code" = **98383**) at (73.06, −15.57, −117.60), `PropQuestBitName("Note_4_1")` = COMPLETE |
| `Note_5_2` "Return to the Ops Deck" | ops2 `QB Filter` **128** → `QB Set` **180**, at (22.00, −8.80, 0.00) near the elevator | ops2 `QB Filter` **1101** (`transmit`) → `QB Set` **1103** (`Note_5_2` = COMPLETE) at (23.83, −8.00, −2.61) |

### The late-game second visit (NOT part of the first pass)

`ops4` also contains the **Ops Override / Command Center** step the player
returns for *after* the Command deck:

- `Ops Comp` **460** at (75.35, −8.37, −116.45) → `OpsCom QB Set` **532**
  (`opscom`), `Message Trap` 502, `EmailTrap` 569, `QB Set` **388**
  (**`Note_6_3` = COMPLETE**, "Go to the Command Center on the Ops Deck") and
  `QB Set` **879** (**`Note_4_9` = INCOMPLETE**, "Go to the Engine Core on the
  Engineering Deck").
  `Ops Comp` 460 inherits from a **`COM Plot Items`** template branch, i.e. it
  is authored as a *Command-deck* plot item that happens to live on Ops.
- `Card slot` **493** at (68.36, −9.14, −113.15) (`TweqLockedButton`,
  `PropLocked(true)`), fed by `New Tripwire` 458 — the card-locked approach to
  that room. The card that opens it is the **Ops Override access card**, which
  the public walkthroughs place on the **Command deck bridge**, not on Ops. No
  object in `ops1..ops4` was found holding it.

A first-visit playtest should **not** be expected to reach `Note_6_3` /
`Note_4_9`. If it does, something has mis-gated.

## Items, codes, and where they are genuinely learned

| Item / code | Genuine source (data-verified) | Purpose |
| --- | --- | --- |
| **Chip A** — "A Quantum Simulation chip" (obj 554, ops2) | **inside `Red Assassin` 254** (tmpl −3398, model `redass`) at (63.39, −8.02, 10.98) | Consumed by `SimComp_1` (ops3) |
| **Chip B** — "A Linear Simulation chip" (obj 438, ops4) | **inside `Red Assassin` 436** (tmpl −3398) at (67.48, −9.02, −103.53) | Consumed by `SimComp_2` (ops4) |
| **Chip C** — "An Interpolated Simulation chip" (obj 840, ops3) | **inside `Docile` 125** (tmpl −1073, also model `redass`) at (17.18, −8.02, 47.95) | Consumed by `SimComp_3` (ops4) |

All three carriers also SwitchLink a **`Ninja Run Trap`** (ops2 388, ops3 807,
ops4 445) — they are authored to flee, so a session that lets one escape may
strand a chip.
| **Code 13433** (weapons lockup) | ops3 audio log **1349** (deck 4 / log 7, Bronson "No shirkers"): *"I've changed the weapons lockup code to 13433."* | ops4 `Keypad` **333** |
| **Code 98383** (MedSci2 sub-armory) | ops4 audio log **525** (deck 4 / log 11, Bronson) | A *back-reference* to MedSci; nothing on Ops uses it |
| **Security Card** (obj **707**, ops4) | world-placed at (49.66, −12.34, −132.19) — **not** in a container | `Card slot` **619** (50.67, −11.74, −133.36) → `Sec Station Door` 571; `Card slot` **620** (54.24, −11.74, −139.45) → `Sec Station Door` 617. Both also have `Unlock Trap` 479 as an alternate opener |
| **Crystal Shard** ×2 (ops3 **748**, **773**) | 748 at (25.24, −9.48, 49.35); 773 at (37.02, −9.48, 143.87). Both `weapontype crystalshard`, `PropObjState(Unresearched)`, `ResearchableScript` | First Exotic melee weapon; SHODAN's email "An elegant weapon" is the authored brief |

Contained items (chips, the logs marked "inside" below) must be looted through
the corpse/container MFD. Finding editor coordinates and calling a debug
give/teleport is not genuine play.

## Audio logs on the deck (`PropLog { deck: 4, log: N }`)

| Log | Speaker / subject | Object | Where |
| --- | --- | --- | --- |
| 1 | **Yount**, "Sim Units" | ops2 **67** | (23.60, −7.93, 10.08) — arrival crates by the XERXES core |
| 16 | **Malick**, "My red friends" — the three override chips are with three cyborgs "dressed in red" | ops2 **544** | (45.90, −9.57, 4.54) |
| 10 | **Siddons**, "Civil war" | ops2 **545**, **inside `Desk #2` 383** | (69.93, −8.85, −5.70) |
| 15 | **Korenchkin**, "Everything old…" — the assassin-cyborg programme | ops2 **547**, **inside `Locker` 546** | (54.99, −14.80, 58.04) |
| 18 | **Siddons**, "Bad feeling…" | ops2 **1090** | (65.23, −9.97, 121.44) |
| 19 | **Suarez**, "Let's do it" | ops2 **1093** | (68.44, −9.57, 24.13) |
| 32 | Chemical manifest (Ops storage closet 089) | ops2 **1125** | (58.57, −8.57, 137.93) |
| 5 | **Malick**, "Good bye" — Bronson kills him mid-log | ops2 **1495** | (47.71, −15.97, 69.47) |
| 2 | **Malick**, "Bronson" — he hacked two Sim Units | ops3 **825**, **inside `Desk #2` 151** | (23.99, −8.88, 144.85) |
| 3 | **Bronson**, "Sabotage" | ops3 **826** | (24.27, −5.57, 78.95) |
| 9 | **Wood**, "Crystal gifts" — the two shards | ops3 **828** | (−4.37, −7.95, 52.89) |
| **7** | **Bronson**, "No shirkers" — **code 13433** | ops3 **1349** | (18.27, −9.57, 34.02); carries `Note_4_6` |
| 20 | **Bayliss**, "What gives?" | ops3 **1357** | (−16.47, −12.77, 128.82) |
| 4 | **Bronson**, "Martial law" | ops4 **464**, **inside `Locker` 364** | (74.06, −8.80, −59.62) |
| **6** | **Malick**, "Sim Unit 3" — the booby trap | ops4 **465** | (6.32, −9.57, −10.03); carries `Note_4_5` |
| 8 | **Diego**, "Cease and desist" | ops4 **466**, **inside `Desk #2` 378** | (47.33, −13.28, −138.35) |
| **11** | **Bronson**, "MedSci armory code" — **98383** | ops4 **525** | (73.06, −15.57, −117.60); carries `Note_4_1` |
| 12 | **Bronson**, "Resist" — her death log | ops4 **526**, **inside `OPS Female Corpse` 146** | (47.93, −11.78, −140.96) |
| 14 | **Suarez**, "Don't stop" | ops4 **827** | (34.74, −10.77, −51.04) |
| 13 | **Delacroix**, "The main elevator" — reprogram the Sim Units *and* power the deck-5 transmitter so the bridge elevator works | ops4 **831** | (75.47, −10.77, −102.49) |

Log text lives in `Data/res/strings/LEVEL04.STR` (`LogTextN`), SHODAN's deck-4
emails in the same file (`EmailTextN`). Emails present in the data: 1 "Fix the
Sim Units", 3 "Destroy my enemies", 6 "An elegant weapon" (crystal shard), 7
"Our alliance" (approaching a Sim Unit), 8 "The cancer" (all units done, Rec
already finished → deck 6), 9 "I am pleased" (all units done → Recreation), 10
"Well done" (second visit; self-destruct code 94834).

## Known-tricky mechanics on this deck

- **`ObjConsumeButton` chip insertion.** The Sim Units are *not* hacked. Each is
  an `ObjConsumeButton` with a `PropConsumeType` naming its chip — the same
  mechanism as Engineering's Card Box / Systems Monitoring Unit, which is known
  to work. A Sim Unit that accepts the wrong chip, or that fires its QB chain
  without consuming an item, is a bug.
- **Overridden button scripts.** Because ops2's visible bulkhead buttons are
  demoted to `BaseButton` and the level change lives on a hidden co-located
  object, a port that resolves scripts from the template instead of the
  entity-level `PropScripts { inherits: false }` would let the player skip the
  `ShodanRoom` gate entirely. Verify that pressing 185 / 404 **before** the
  reveal does nothing.
- **`TrapDestroy` on the return path.** `KillBulk1` 1002 destroys the ops1
  changer after the reveal. If the port does not implement `TrapDestroy`, ops1
  will stay reachable — cosmetically harmless, but it means the one-way design
  is not being exercised.
- **Replicators.** ops2 `RepBase` **461** / `RepScreen` **462**; ops3 `RepBase`
  **118** / `RepScreen` **127** and `RepBase` **693** / `RepScreen` **694**. The
  walkthroughs describe one Ops replicator as hackable for a Recycler; hacking +
  nanite economy landed recently (#545, #546), so these are worth exercising.
- **Security computers and cameras.** `Security Comp` in ops2 (133, 346), ops3
  (266, 271, 346) and ops4 (268, 273, 291, 346); `Security Camera` in ops2 (339,
  350) and ops3 (268, 273, 350). Camera alert → security level is a real
  mechanic on this deck.
- **Turrets.** ops3 `Slug Turret` **1167**, **1170** (`S$turret`); ops4 `Laser
  Turret` **281**, **284**, **328**, **335**, **700**. ops3 also has a `Laser
  Turret Corpse` 1260 (a destroyed prop, not a threat).
- **Sim Unit 3 spider trap.** Malick's log 6 warns of it. `New Tripwire` **602**
  at (−9.89, −15.18, −9.49) opens `Grate` 391 and `Grate` 753 and fires
  `AI Signal Trap` **612** — the arachnid ambush — alongside completing
  `Note_4_5`. If the grates or the AI signal do not fire, the encounter is
  silently missing even though the objective ticks.
- **Egg/grub set pieces.** `Floor Egg Tripwire` (template −1651) is used heavily
  in all three big sectors (ops2 ×8, ops3 ×17, ops4 ×5). ops2 also has a
  `Midwife` (**352**) and `Midwife Shot` (1505) — the egg-guarding set piece.
- **Grav lifts.** ops4 `Lift 1` **572** (`BaseElevator`) at (45.30, −10.94,
  −106.00) with call buttons **599** (45.80, −13.75, −104.64) and **600**
  (45.80, −8.97, −104.64) on `Elevator Path` 594/595; ops3 `Cargo Lift` **1035**
  on paths 1039/1040 with buttons 1041/1042. Vertical traversal is required.
- **Crystal Shard research.** Both shards ship `PropObjState(Unresearched)` and
  `ResearchableScript`; researching needs the right chemical, so the Ops
  chemical store (log 32's manifest) is thematically linked.

## What a valid playtest must cover, per mission

### ops2 (Ops B) — the hub
1. Arrival at loc 22 via the real elevator MFD (or a documented frontier
   resume), with inventory and quest state from deck 3 preserved.
2. `Note_3_4` ("Get to Deck 4 to meet Dr. Polito") is active on arrival.
3. Physically walk the hub, loot the arrival crates, take **Yount log 1**
   (object 67).
4. Confirm bulkhead buttons **185** and **404** are inert before the reveal.
5. Frob **361** and transition to ops1 loc 100.
6. After the reveal: return, confirm 185 and 404 now work, and confirm 361 has
   been destroyed / no longer transitions.
7. Reach Systems Administration, kill `Red Assassin` **254**, and loot **Chip
   A** through its container MFD.
8. Optional but valuable: replicator 461, `Midwife` 352 + its eggs, the
   hackable crates 1540/1541.

### ops1 (Ops A) — the reveal
1. Walk in from (16.60, −7.90, 4.30) under normal movement.
2. Trip **204**: the cutscene set (SHODAN screens, force doors, rumblers) plays.
3. `ShodanRoom` becomes set; `Note_2_1`, `Note_3_4`, `Note_1_14` complete.
4. Walk back out past **277**: SHODAN's "Fix the Sim Units" email arrives and
   **`Note_4_2` becomes active**. This must be observed, not assumed.
5. Return through `Bulk_On_Button` **285** to ops2 loc 100.

### ops3 (Ops C)
1. Enter via ops2 button 185 → loc 200 (or 409 → loc 400).
2. Data Storage: deal with `Docile` **125** and loot **Chip C** (840).
3. Take at least one **Crystal Shard** (748 or 773) and observe SHODAN's
   "elegant weapon" email; research it if the research path is being exercised.
4. Loot and **read** Bronson log **1349** so **`Note_4_6` (13433)** becomes
   active *before* ops4's keypad is used.
5. Reach `SimComp_1` **810** at (39.55, −8.40, 166.31), insert **Chip A**,
   confirm the chip is consumed, `Comp1` is set, and the unit's model swaps
   (`simqua` → `simover`).
6. Survive the Slug Turrets 1167/1170 and the Mess Hall / cold-storage set
   pieces.
7. Return via 742 (loc 200) or 927 (loc 400) with state intact.

### ops4 (Ops D)
1. Enter via ops2 button 404 → loc 300.
2. Enter **13433** on `Keypad` **333** using the code learned from log 1349;
   `Note_4_6` completes and `Ops Door` 331/332 open.
3. Kill `Red Assassin` **436** and loot **Chip B** (438).
4. Loot Bronson log **525** → `Note_4_1` completes (98383 for MedSci2).
5. Loot Malick log **465** → `Note_4_5` becomes active (Sim Unit 3 trap).
6. Take the **Security Card** 707 and use it at `Card slot` **619** and/or
   **620** to open the Sec Station doors.
7. Insert **Chip B** into `SimComp_2` **437** at (49.41, −12.40, −141.12)
   → `Comp2`.
8. Ride grav lift 572 / descend to Power Ops lower, trip the Sim Unit 3 trap
   (`Note_4_5` completes), and insert **Chip C** into `SimComp_3` **444** at
   (−9.94, −14.80, −7.51) → `Comp3`.
9. On the third unit: **`Reprogram` is set, `Note_4_2` completes**, the XP trap
   pays out, and one of the SHODAN emails 8/9 arrives depending on `transmit`.
10. Return to ops2 and take the main elevator onward (Recreation if `transmit`
    is unset).

## Genuine-play acceptance gate for Operations

Mark Operations **PASS** only if one reviewed session (or a frontier-backed
sequence) demonstrates all of:

1. Real elevator arrival into `ops2` loc 22 with deck-3 state preserved.
2. Physical traversal to the ops1 bulkhead and a real transition to `ops1`.
3. The reveal cutscene fires from tripwire 204 under normal movement, and
   `ShodanRoom` / `Note_2_1` / `Note_3_4` / `Note_1_14` change accordingly.
4. Walking back out delivers SHODAN's "Fix the Sim Units" email and activates
   **`Note_4_2`**.
5. The ops3 / ops4 bulkheads are demonstrated inert *before* and working
   *after* the reveal.
6. All three chips are **looted from their carriers' containers** (254, 125,
   436) — not spawned, not found on the floor.
7. Each chip is inserted into its **matching** Sim Unit through the real
   `ObjConsumeButton` frob, and each sets its `Comp` bit and swaps the unit's
   model.
8. `Note_4_2` completes only on the third insertion, with `Reprogram` set and
   an XP award, and exactly one of email 8 / email 9 delivered.
9. Bronson log 1349 is looted and its transcript visibly shows **13433**
   *before* keypad 333 is used; `Note_4_6` activates then completes.
10. Malick log 465 is looted (`Note_4_5` active) before the Sim Unit 3 trap, and
    `Note_4_5` completes at the trap.
11. Bronson log 525 is looted and `Note_4_1` completes.
12. At least one cross-bulkhead round trip (ops2 ↔ ops3 and ops2 ↔ ops4) with
    chips and quest state surviving both transitions.
13. Screenshots and `data.json` steps show each interaction and its consequence.
    Quest-state assertions alone are not proof.

A session that reaches ops3/ops4, kills things, and reports "deck explored" but
never inserts a chip is **shallow, not a pass**. Typing 13433 without reading
log 1349, debug-giving a chip, injecting a quest bit, or messaging a Sim Unit
directly all invalidate the run.

## Engine watch-points / likely feature-gap boundaries

- **Log quest-bit metadata is the same suspected blocker as Engineering.** Three
  Ops logs carry `PropQuestBitName` directly on the disc entity — ops3 **1349**
  (`Note_4_6`), ops4 **465** (`Note_4_5`), ops4 **525** (`Note_4_1`) — with no
  SwitchLink fallback. Engineering's walkthrough already records that the
  current `MediaGui` / `CollectLog` path displays a log but does not apply that
  metadata. If reading these logs does not move their objectives, report the
  objective-integration failure; do not inject the bit.
- **The `ShodanRoom` gate depends on entity-level script override.** See
  "Known-tricky mechanics". This is the highest-value thing to test first,
  because if it fails open the whole deck's intended order collapses, and if it
  fails closed the deck is unfinishable.
- **`TrapDestroy` (KillBulk1 1002)** — verify whether it is implemented.
- **`CS9_EggsandGrubs` / `TransluceInOutHolo` / `TrapRouter` / `TrapDelay`** —
  the ops1 reveal set piece is built from these. A missing script here would
  most likely present as "the cutscene did not play" while the QB sets still
  fire from tripwire 204 (they are separate SwitchLinks), so **check the quest
  bits separately from the visuals**.
- **`ObjConsumeButton` with three distinct `PropConsumeType` values** — the
  Engineering path exercised exactly one. Wrong-chip acceptance is a plausible
  regression.
- **`turret` and `Security Comp` scripts** are exercised heavily here.
- ops3 and ops4 both contain `Regen_Hologram` objects (ops2 1082, ops3 855,
  ops4 1082) — the bio-reconstruction machines. Death/respawn behavior on this
  deck depends on them.

## Data-vs-walkthrough discrepancies (trust the data)

1. **"Only bulkhead 41 is open at first."** The public walkthroughs state this
   as a soft observation. The data shows *why*: `ShodanRoom` filters 995/998.
   Confirmed, with a mechanism.
2. **"Hack the three Simulation Units."** Several summaries say the units are
   *hacked*. In the shipped data they are `ObjConsumeButton`s that **consume a
   chip** (`PropConsumeType "Chip A" / "Chip B" / "Chip C"`). There is no
   hacking script on `SimComp_1/2/3`. The lore word is "reprogram"; the
   mechanic is insertion.
3. **Sim Unit distribution.** The SS2 wiki's objectives section claims one unit
   per sector B/C/D. The data agrees with the walkthroughs instead: **none in
   ops2**, **one in ops3** (`SimComp_1`), **two in ops4** (`SimComp_2`,
   `SimComp_3`).
4. **"No keycard is found on Operations."** The data contradicts this: `ops4`
   contains a world-placed **`Security Card` 707** at (49.66, −12.34, −132.19)
   which drives `Card slot` 619/620 → the two `Sec Station Door`s. What is
   *absent* from ops1..ops4 is the **Ops Override access card** for `Card slot`
   493 — consistent with it being a Command-deck item.
5. **Third assassin's name.** Chip C's carrier is object **125 named `Docile`**
   on template **−1073**, not the `Red Assassin` template (−3398) used by ops2
   254 / ops4 436 — but it uses the same `redass` model and the same
   `Ninja Run Trap` wiring, so it *is* the third red assassin. Consistent with
   the walkthroughs' note that the Data Storage assassin is usually killed by
   the exploding-barrel trap rather than fought. **A playtest must search by
   both names and by both template ids**, not just "Red Assassin".
6. **Crystal shard count and placement.** Wood's log says "two gifts"; the data
   has exactly two shards and **both are in `ops3`** (sector C), matching
   sshock2 and contradicting any account that places one elsewhere on the deck.
7. **`map_ops1.html` is a placeholder.** Sector A has no retail automap, which
   is consistent with `ops1.mis` being a 225-object cutscene room.
8. **No living named NPC on this deck.** Bronson, Malick, Wood, Delacroix and
   Korenchkin appear only as logs and corpses in the data. Do not expect a
   Miranda Wood encounter.
9. **Second visit is out of scope for a first pass.** `Note_6_3` / `Note_4_9` /
   the `opscom` bit hang off `Ops Comp` 460 behind a card slot with no card on
   this deck.

## Open questions

- **Where is `Note_4_2` re-activated if a player somehow reaches ops3/ops4 first?**
  Only `EmailTrap` 279 in `ops1` sets it. If a save resumes mid-deck without it,
  the objective list will be wrong; a frontier resume should carry it.
- **`Card slot` 493's SwitchLink target is `Ops Crew` 187, not a door.** That is
  unusual; the actual Command-Center door wiring for the second visit was not
  fully traced. Worth a follow-up `cargo dq` pass before anyone tests the
  second visit.
- **`Note_4_3`, `Note_4_4`, `Note_4_8`** ("Go to the Command Deck", "Go to the
  Recreation Deck", "Create a data access channel from the Command Center") have
  strings in `NOTES.STR` but **no `TrapQBSet` in ops1..ops4 was found setting
  them**. They are presumably driven from `rec1`/`command1`. Do not treat their
  absence on Ops as a bug.
- Which of the three Ops replicators (ops2 461, ops3 118, ops3 693) is the
  Recycler-hack one was not determined from the data.

## Sources

- Mission wiring: `cargo dq entities ops1.mis|ops2.mis|ops3.mis|ops4.mis …`
  (2026-07-24), especially objects 72, 114, 204, 264, 277–279, 285, 296, 297
  (ops1); 128, 185, 254, 352, 361, 404, 409, 461, 537, 554, 992, 995, 998,
  999, 1001–1004, 1092, 1101, 1103 (ops2); 125, 685, 742, 810, 825–828, 840,
  927, 1349, 1357, 1368–1377 (ops3); 333, 340, 436–438, 444, 448–456, 460,
  464–466, 483, 484, 489, 493, 525, 526, 532, 556, 602, 619, 620, 707,
  811–819, 827, 828, 831, 835, 839, 879 (ops4), plus their SwitchLinks,
  `Contains` links and quest-bit traps.
- Retail text: `Data/res/strings/LEVEL04.STR` (deck-4 SHODAN emails and audio
  logs, including 13433 / 98383 / 94834) and `Data/res/strings/NOTES.STR`
  (ordered `Note_4_*`, `Note_5_*`, `Note_6_*` objective strings).
- Port behavior: `shock2vr/src/scripts/gui/elevator.rs` (`ELEVATOR_STOPS` —
  deck 4 loads `ops2.mis`).
- [sshock2.com full walkthrough](http://www.sshock2.com/ss2walk/) and its
  sector maps [map_ops2](http://www.sshock2.com/ss2walk/map_ops2.html),
  [map_ops3](http://www.sshock2.com/ss2walk/map_ops3.html),
  [map_ops4](http://www.sshock2.com/ss2walk/map_ops4.html)
  ([map_ops1](http://www.sshock2.com/ss2walk/map_ops1.html) is a "no map"
  placeholder).
- [portforward.com — Operations](https://portforward.com/games/walkthroughs/System-Shock-2/Operations.htm)
  — screenshot-by-screenshot route.
- [System Shock Wiki — Operations Deck](https://shodan.fandom.com/wiki/Operations_Deck),
  plus its [Codes](https://shodan.fandom.com/wiki/Codes),
  [Simulation Unit](https://shodan.fandom.com/wiki/Simulation_Unit),
  [Crystal Shard](https://shodan.fandom.com/wiki/Crystal_Shard) and
  [Malick](https://shodan.fandom.com/wiki/Malick) pages.
- [Steam access-code guide](https://steamcommunity.com/sharedfiles/filedetails/?id=161214166).

---

## Campaign findings (verified in play, 2026-07)

Recorded from five playtest sessions and their adversarial reviews. Everything
below was confirmed by walking or by mission data, not inferred.

### The verified ops4 route south

Two sessions concluded ops4's south was unreachable. **It is not.** The jam at
(53.5, −8.60, −27.6) is the **end of a lane, not the boundary of a region**.
From the loc-300 arrival (54.7, −7.9, −21.8):

```
(54.5,-18) -> (50,-19) -> south lane x~53 to (52.8,-8.6,-27.6)
then WEST along z=-27:  (48,-27) (44,-27) (40,-27) (36,-27)
then the x~42-44 corridor SOUTH:
  (42,-38) (42,-42) (42,-46)  [y drops to -9.0]
  (43,-50) (50,-53) (54,-55) (58,-58) (60.5,-59)
ends ~1.5u from Keypad 333 (60.59,-8.83,-59.55)
```

Two snags, both cleared by a **2-unit lateral offset** (not capsule bugs): the
floor pipes at (36.6 / 37.2, −8.4, −34.0) and the x ≈ 44.6 RickPipes.

### Navmesh queries are NOT a reachability oracle here

`ops4` has **846 disconnected walk components** across 2204 cells. A
`cargo bn path show` / `path component` query is confined to the arrival's own
island **by construction**, so it will always "prove" the rest of the deck
unreachable. That is a tautology, not evidence. Additional traps:

- `bench path show` has **no `--bridges` flag` (only `path bench` does).
- `path cell` matches in the **XZ plane only**, so it cannot distinguish the
  Power Ops upper/lower floors.
- Negative-leading coordinates are silently misparsed as flags (issue #571) —
  always use `--from=` / `--to=` with an `=`.

**The player is physics-driven, not navmesh-driven.** Decide reachability by
walking; when a lane jams, strafe laterally 2–6u and retry before concluding
anything is unreachable.

### The grav-lift "trap room" is authored — and has an escape

The force barrier at x = 35.70, z = −106.0 that seals the grav-lift pocket is
**not a softlock**. `New Tripwire` **662** (`ENTER|ONCE|PLAYER`) fires
`Teleport Trap` **656/657/660**, and `TrapTeleport` moves its switch-link
targets — the Force Bars — to the trap position (the barrier sits exactly at
trap 656's position; Force Bar 658's base position is (41.6, −0.4, −81.4)).

The way out is inside the pocket: **`Junction Box` 685 (3 HP) → `Destroy Trap`
664 → destroys Force Bars 545/658/659.** Shoot the junction box.

### Objective activation vs completion are separately authored

For every Ops objective, **activation and completion live on different objects**:

| Objective | Activated by | Completed by |
| --- | --- | --- |
| `Note_4_2` | ops1 `EmailTrap` **279** (on the walk back out) | ops4 `QB Set` **483**, behind the three-comp AND gate |
| `Note_4_6` | ops3 log **1349** (`PropQuestBitName`) | ops4 `QB Set` **489** (keypad 333) |
| `Note_4_5` | — | ops4 `QB Set` **828** (tripwire 602) |

Issue **#568** breaks only the *activation* half (log discs' quest bits are
ignored), which is why an objective can be observed completing while never
having been active. That is the bug, not authored behavior.

### The Sim Unit AND gate

`SimComp_3` **444** → **448** → `TriggerDelay` **449** → QB Filter `Comp 1`
(**450**) → `Comp 2` (**451**) → `Comp 3` (**815**) → `QB Set` **483**
(`Note_4_2` = COMPLETE) + **835** (`Reprogram`).

It is **impossible** to set 483 without all three comps, so observing
`Note_4_2` complete independently corroborates that all three units were
genuinely reprogrammed.

Chip → unit mapping and tweqs: `SimComp_1` **810** Chip A `simqua→simover` ·
`SimComp_2` **437** Chip B `simlin→simover` · `SimComp_3` **444** Chip C
`simint→simover`. `ObjConsumeButton` matches `PropConsumeType` against the
item's `PropSymName` (case-insensitive) and destroys the item on consume.

### Interaction paths that automation gets wrong

- **World pickup is `right_hand.squeeze`** against the crosshair target, via
  `FlatPlayerController::update` → `can_grab_item` → `pick_up`/`StoreItem`.
  Injecting `{Frob}` can never pick anything up.
- **Wield is a double-click** on a use-mode strip slot: two `pointer.pressed`
  1→0 edges within **20 stepped frames** and 24 canvas px. A single click only
  lifts the item to the drag cursor. There is deliberately no separate hand slot.
- **Container loot** (chips, corpse items) must go through the container MFD.
  **Never** `/v1/player/give` a container-held item — it permanently severs the
  `Contains` link (issue #572) and destroys the loot path for every later run.

### Known broken on this deck (open issues)

`#569` world ammo has no colliders (varies per instance — `HE Clip` 752 gets one,
identically-propertied clips do not) · `#573` **a save taken while a camera is
red is permanently unloadable** · `#574` ladder segments without
`PropPhysDimensions` get no collider, so ops4's Rick Ladders can't be gripped
from the top · `#571` / `#572` tooling hazards above.
