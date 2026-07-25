# Walkthrough — rec1 (Recreation, deck 5: crew annex, athletics, the transmitter, and the Command elevator)

Context for the `playtest` agent and the `play-through` reviewer. `rec1` is the
**hub and the only entry point** of the Recreation deck, and it is where the
deck's single plot objective lives: **find the transmitter and turn it on**.
Reaching `rec2` or `rec3`, or riding a lift, is *not* completion. Recreation is
complete only once the **Transmitter Tower has actually been activated** with
the real keypad, setting the `Transmit` quest bit.

This is not a movement script. Observe and react, traverse with bounded
movement, fight or evade, and use the anchors below to identify the intended
objective rather than teleporting between coordinates.

**Positions and positive IDs below are mission-file object identities** from
`cargo dq entities rec1.mis …`. Runtime entity IDs are assigned per launch and
are **not stable** — resolve entities anew each run by name and by the stable
`template_id`, and never hardcode a runtime entity ID.

## What the three Rec missions are

Recreation is one deck split into three map files. The automap room objects
(template `1013`) in each file name the sectors directly:

| File | Sector rooms named in the data | Role | Size |
| --- | --- | --- | --- |
| `rec1.mis` | `0 Upper hotel`, `3 Lowerhotel`, `4 Athletics`, `5 Rec`, `2 Medhalls`, `1 bballtop` | **Hub.** Main-elevator arrival, crew annex / crew quarters, athletic sector, the **Transmitter Tower**, the pool + basketball court, and the **Command-deck elevator**. All four outbound bulkheads and both elevators live here. | 1127 objects |
| `rec2.mis` | `0 Garden`, `1 Lower garden`, `3 tunnels`, `4 Rec2`, `5 Lounge`, `6 Near Mess`, `7 Kitchen`, `7 Dining`, `8 UpperLounge`, `9 Upper bar`, `Bar #2`, `2 low dining` | Garden, maintenance tunnels under the garden, mess hall / kitchen, lounges and bars. Holds the **Rec Crew Access Card**. | 1034 objects |
| `rec3.mis` | `0 Intro Mall`, `3 Upper Mall`, `4 Lower Mall`, `1 Up Cathouse`, `6 Down Cat`, `5 Security`, `6 Theater`, `7 Casino` | The mall, casino, theater, the sensual-sim ("cathouse") booths, and the Rec security station. Mostly optional loot / upgrade content. | 796 objects |

**Only `rec1` contains plot-critical wiring.** `rec2` and `rec3` contribute two
side objectives (`Note_5_6`, `Note_5_8`) and the Rec Crew Access Card; nothing
in either file sets `Transmit`.

## Arrival, exits, and the connector graph

Recreation is deck 5 on the main inter-deck elevator.
`shock2vr/src/scripts/gui/elevator.rs`'s `ELEVATOR_STOPS` is `["eng1.mis",
"medsci1.mis", "hydro2.mis", "ops2.mis", "rec1.mis"]`, so **"Recreational (5)"
loads `rec1.mis`**, arriving at **loc 22**.

| Purpose | Stable mission object | Position | Destination |
| --- | --- | --- | --- |
| **Main elevator panel** | `Master Elevator Button` **74** (tmpl −1723, `S$ElevatorButton`) | (−3.91, −3.60, −127.50) | Five-deck elevator MFD; Rec arrival marker is **loc 22**, id **273** @ (−3.91, −3.60, −127.48) |
| rec1 → **rec2** (west bulkhead) | `Bulk_On_Button` **546** (tmpl −3622) | (−12.50, −3.60, −66.32) | `rec2`, loc **200** |
| rec2 → rec1 | `Bulk_On_Button` **273** | (−12.51, −3.60, −81.25) | `rec1`, loc **200** (marker 240 @ (−12.50, −3.60, −71.67)) |
| rec1 → **rec2** (east bulkhead) | `Bulk_On_Button` **555** | (27.41, −3.60, −74.24) | `rec2`, loc **300** |
| rec2 → rec1 | `Bulk_On_Button` **507** | (27.40, −3.60, −87.27) | `rec1`, loc **300** (marker 244 @ (27.40, −3.60, −79.57)) |
| rec1 → **rec3** (east bulkhead) | `Bulk_On_Button` **274** | (74.35, −3.60, −98.11) | `rec3`, loc **500** |
| rec3 → rec1 | `Bulk_On_Button` **381** | (60.24, −3.60, −98.01) | `rec1`, loc **500** (marker 252 @ (69.03, −3.60, −98.10)) |
| rec1 → **rec3** (south bulkhead) | `Bulk_On_Button` **543** | (28.18, −3.60, −187.41) | `rec3`, loc **100** |
| rec3 → rec1 | `Bulk_On_Button` **387** | (24.43, −3.60, −187.80) | `rec1`, loc **100** (marker 277 @ (22.85, −3.60, −187.40)) |
| **rec1 → `command1` (Command deck)** | `Level Change Button` **664** (tmpl −2473, entity-level `PropScripts { ["SimpleLevelChangeButton","TweqDepressable"] }`) | (36.13, −3.40, −115.21) | **`command1`**, loc **56** |
| `command1` → rec1 (return) | — | arrival marker **279**, `PropStartLoc(65)` @ (36.13, −3.40, −115.20) | inside the same elevator car |

`rec2 ↔ rec3` connect directly to each other as well (`rec2` 540 → `rec3` loc
400; `rec3` 582 → `rec2` loc 400), so the three files form a ring: `rec1` is the
only one with the main elevator, but the sectors are otherwise freely
traversable once reached.

### The Command elevator is double-gated (the most important structural fact)

The `Level Change Button` 664 that goes to `command1` sits **inside an elevator
car sealed behind `Double Elevator Door`s**. The car is opened from outside by
`Big_Orange_Button` **653** @ (33.93, −3.30, −112.69), whose SwitchLinks form an
authored AND gate on two cross-deck quest bits:

```
Big_Orange_Button 653
  ├─ QB Filter 778  (Transmit)   → QB Filter 655 (reprogram)
  │                                   → Double Elevator Door 650 @ (35.6,−3.4,−113.0)
  │                                   → Double Elevator Door 651 @ (35.6,−3.4,−114.6)
  │                                   → Trigger Delay 659 (500 ms) → doors 497, 652
  ├─ Anti-QB Trigger 782 (Transmit)  → Anti-QB 890 (Reprogram) → Message Trap 756
  ├─ Anti-QB Trigger 783 (reprogram) → QB Filter 891 (Transmit) → Message Trap 780
  └─ Anti-QB Trigger 893 (Transmit)  → QB Filter 892 (reprogram) → Message Trap 784
```

So the doors open **only when both `Transmit` (set by this deck's transmitter)
and `Reprogram` (set on Operations after all three Sim Units) are set**; each of
the three "not yet" combinations plays its own `Message Trap` refusal instead.
`Big_Orange_Button` **512** @ (36.19, −3.30, −111.99) is the button *inside* the
car and opens the same doors unconditionally (it is the way back out).

**Rec and Ops are order-independent by design** — see `ops.md`'s `Gone To Rec` /
`Not Gone 2 Rec` pair, keyed on this same `transmit` bit. Whichever deck is
finished second is the one that opens this elevator.

## Codes and required items

| Item / code | Genuine source (data-verified) | What it opens / is for |
| --- | --- | --- |
| **Transmitter code `14106`** | **Not in any audio log.** `Transmitter Tower Off` **89** carries `PropKeypadCode(14106)` directly. Log 14 (Yang, "Barricaded in") only says the code was split across the deck's *art terminals*. | The transmitter itself (see below) |
| **`11111`** (crew annex, level 2) | `rec3` audio log **672** (deck 5 / log 8, Rosenberg "Looking out for #1") @ (78.54, −3.75, −204.44); carries `PropQuestBitName("note_5_8")` | `rec1` `Keypad` **1955** (`PropKeypadCode(11111)`) @ (32.41, 4.69, −85.77) → `Sci Med Door` **1544** @ (34.10, 4.80, −85.31) and `QB Set` **490** (`note_5_8` = COMPLETE). Behind it: the **`Viral Prolif`** (object **775**, tmpl −29) @ (40.02, 3.05, −83.19) |
| **`34093`** (garden maintenance tunnel) | `rec2` audio log **528** (deck 5 / log 16, Cortez "Under the garden") @ (10.05, −8.77, −2.57); carries `note_5_6` | `rec2` `Keypad` **122** — see `rec2.md` |
| **`12345`** | No log found that teaches it (see Open questions) | `rec1` `Keypad` **270** @ (−4.14, −3.86, −163.74) → `Floor Hatch` **626** @ (−3.88, −4.15, −162.06) |
| **`Crew_Card2`** — "Crew 2 Athletic Section key card" (object **80**, `PropKeySrc` region **32768**) | World-placed in the athletic sector @ (73.70, 3.85, −120.58). Frobbing it sets quest bit `ladycrewcard` (`FrobQB`), and `Simple QB Trigger` **582** @ (73.54, 3.85, −120.15) pays `Experience Trap` **746** | `Card slot` **125** @ (−1.46, −3.40, −198.35) → `Residential Door` **302** @ (−0.95, −3.10, −196.60), plus `Experience Trap` 496 |
| **`Rec Crew Key`** — "Rec Crew Access Card" (`rec2` object **996**, region **32**) | `rec2` @ (98.00, −7.94, −52.83) — see `rec2.md` | Region-32 card slots across **all three** Rec maps: `rec1` **293** @ (12.06, −3.11, −145.32), **2016** @ (10.48, −3.11, −148.42), master-key slots **295** @ (4.54, −3.11, −135.67) and **2014** @ (3.35, −3.11, −140.03); `rec2` 623 / 1004; `rec3` 295 / 469 |

## The transmitter — the deck's real objective

| Object | Position | Notes |
| --- | --- | --- |
| **`Transmitter Tower Off` 89** (tmpl −1480) | **(−11.31, 3.20, −214.75)** | Entity-level `PropScripts { ["KeypadUnhackable"], inherits: false }`, `PropKeypadCode(14106)`, `PropHackDiff { success_chance: −2000, cost: 900 }` — i.e. **deliberately un-hackable; the code must be entered** |
| `Radar_Dish` **668** | (−13.84, 1.60, −217.87) | `TrapTweq` — the visible dish that starts turning |

Activating 89 fires, in one shot:

| Target | Effect |
| --- | --- |
| `QB Set` **482** | `note_5_4` = **COMPLETE** ("Activate transmitter with code hidden in art terminals") |
| `QB Set` **483** | `note_5_7` = **COMPLETE** ("Find the transmitter and activate it") |
| `QB Set` **777** | **`Transmit` = INCOMPLETE** — this raw-1 value *is* the "transmitter is on" marker, the same counterintuitive convention Hydroponics uses for its regulators |
| `Experience Trap` **495** | +20 cyber modules |
| `Radar_Dish` 668, `Sound Trap` 839 | the dish spins, the transmission sound plays |
| `Trigger Delay` **158** (8 s) | → `DirectMonsterGen` **1393** @ (−21.66, −2.54, −159.87) and **1394** @ (−0.52, −3.38, −185.55) — **Yang's rigged security alert** (log 14: *"I've also rigged up the tower to set off a security alert in case somebody else tries to tamper with it"*), plus the branch below |

The 8-second delay then branches on **`reprogram`** (Operations' Sim Units):

- `QB Filter` **162** (`reprogram` set) → `Sound Trap` 796 + `Trigger Delay` 307
  (7.5 s) → `EmailTrap` **120**: deck-5 **email 6 "The cancer"** — *"I have
  activated the primary elevator shaft… take it to deck 6"* — and sets
  **`note_5_3` = INCOMPLETE** ("Go to the Command Deck").
- `Anti-QB Trigger` **161** (`reprogram` unset) → `Sound Trap` 813 +
  `Trigger Delay` 814 (7.5 s) → `EmailTrap` **118**: deck-5 **email 10 "My
  revenge"** — *"Now return to Ops and reprogram the Simulation Units"* — and
  sets **`note_5_2` = INCOMPLETE** ("Return to the Ops Deck").

A playtest must step at least **~16 s / 960 fixed frames** after activation
before reporting a missing email or objective.

## Objective activation on arrival

Two **room** objects (`BaseRoom`/`CoreRoom`, `PropMapLoc(5)`, no world position —
they fire on player room entry) drive everything the player is told when they
step off the elevator:

| Room object | Wiring | Consequence |
| --- | --- | --- |
| **`QBopsdone` 167** | → `OnceRouter` **29** → `QB Set` **921** | **`note_4_4` = COMPLETE** ("Go to the Recreation Deck") |
| | → `Router` **26** → `QB Filter` **168** (`reprogram`) → `EmailTrap` **171** + `TrapSlayer` 27 | deck-5 **email 9** "Transmitter" (the *Ops already done* wording) and **`Note_5_7` = INCOMPLETE** |
| | → `Router` **26** → `Anti-QB Trigger` **170** (`reprogram`) → `EmailTrap` **172** + `TrapSlayer` 27 | deck-5 **email 11** "Transmitter" (the *Ops not yet done* wording) and **`note_5_7` = INCOMPLETE** |
| **`recopsQB` 166** | → `QB Filter` **173** (`shodanroom`) → `Unlock Trap` **155** → unlocks `Simple Button` **124** @ (−6.83, −3.11, −139.47) | opens the route south through `Sci Med Door`s **1922** / **1923** @ (−8.88, −3.10, −136.5 / −138.9) |
| | → `Anti-QB Trigger` **111** (`shodanroom`) → `EmailTrap` **119** | deck-5 **email 2**, Polito: *"Get back in that elevator and come to deck 4."* — the "you came to Rec too early" nag |

`Buffy the Trap Slayer` **27** (`TrapSlayer`) destroys `Router` 26 after the
first pass, so the transmitter briefing email arrives exactly once.

## Quest bits that gate or record progress on Recreation

| Bit | Set by | Meaning for a playtest |
| --- | --- | --- |
| **`shodanroom`** | `ops1` (the Polito's-office SHODAN reveal) | **Pre-set it if you resume mid-campaign.** Unset ⇒ Polito's "go back to deck 4" email, and (in the original) the south route stays locked |
| **`reprogram`** | `ops3`/`ops4` after all three Sim Units | Selects which SHODAN emails play, sets `note_5_2` vs `note_5_3`, and is **one half of the Command-elevator AND gate** |
| **`Transmit`** | **`rec1` `QB Set` 777**, from the transmitter | The Recreation deliverable; the other half of the Command-elevator gate; read back on Ops |
| `ElevState` | earlier decks | Main-elevator gating. In this port `is_floor_available` treats raw 1 as "deck ≥ 2 only" and everything else as open, so deck 5 is reachable even from an unset state — **do not treat "I could click Recreational (5)" as evidence of campaign progress** |
| `note_4_4` | `rec1` `QB Set` 921 on arrival | "Go to the Recreation Deck" completes |
| `note_5_7` | activated by `EmailTrap` 171/172, completed by `QB Set` 483 | "Find the transmitter and activate it" — the headline objective |
| `note_5_4` | activated by logs 108 / 109, completed by `QB Set` 482 | "Activate transmitter with code hidden in art terminals" |
| `note_5_5` | activated by log **1983**, completed by `QB Set` **481** ← `Light_button` **77** | "A circuit breaker for the basketball court lights is near the pool" |
| `note_5_8` | activated by `rec3` log 672, completed by `QB Set` **490** ← `Keypad` **1955** | "An exotic weapon is on level 2 of the crew annex, code 11111" |
| `note_5_6` | activated by `rec2` log 528, completed by `rec2` `QB Set` 503 | "The code for the garden maintenance tunnel is 34093" |
| `note_5_2` / `note_5_3` | `EmailTrap` 118 / 120 after the transmitter | "Return to the Ops Deck" / "Go to the Command Deck" — mutually exclusive, chosen by `reprogram` |
| `ladycrewcard` | `Crew_Card2` 80 (`FrobQB`) | First-pickup XP marker only |

## The basketball-court lights side objective (`note_5_5`)

Log **1983** (deck 5 / log 11, Yang "Blackouts") @ (−7.83, −3.90, −193.37) says
to re-set the circuit from the breaker by the pool. In the data the breaker is
**`Aux Power W/Out Battery` 807** @ (33.60, −2.80, −236.44), which SwitchLinks
`Message Trap` 825 and **`Light_button` 77** @ (32.79, −2.79, −235.85).
`Light_button` 77 then drives sixteen light objects **and** `QB Set` **481**
(`note_5_5` = COMPLETE).

The object name is literally "Aux Power **W/Out Battery**" — the retail flow is
generally described as needing a power source. Whether the port requires a
battery item here was **not verified**; treat a refusal at 807 as
"needs power / unimplemented", not as a hard blocker.

## Audio logs in `rec1` (`PropLog { deck: 5, log: N }`)

| Log | Speaker / subject | Object | Position |
| --- | --- | --- | --- |
| **1** | **Cortez**, "Transmitter units" — the code is split across the deck's art terminals | **109** | (−7.08, −3.62, −106.69); carries `note_5_4` |
| 2 | Delacroix, "Friends and enemies" | 1756 | (7.27, −3.39, −168.31) |
| 3 | Murdoch, "What's going on?" | 1862 | (−1.44, −3.81, −178.10) |
| 4 | Murdoch, "Ick" (swarm eggs near the observation chambers) | 683 | (−14.66, −4.98, −76.74) |
| 5 | Cortez, "The mall" — *"Stay out of the mall if you can. It crawls."* | 679 | (20.14, −4.23, −150.82) |
| 7 | Delacroix, "Turn on transmitter" | 678 | (17.59, −4.83, −144.61) |
| **11** | **Yang**, "Blackouts" — the pool-side breaker | **1983** | (−7.83, −3.90, −193.37); carries `note_5_5` |
| **14** | **Yang**, "Barricaded in" — *"I got the art terminals wired up to display the fragmented dish alignment"* + the security-alert rig | **108** | (−8.17, −3.97, −157.70); carries `note_5_4` |
| 15 | Yang, "Victory!" | 622 | (36.40, −4.50, −150.81) |

Log text lives in `Data/res/strings/LEVEL05.STR` (`LogTextN`), SHODAN's deck-5
emails in the same file (`EmailTextN`). Objective strings are in
`Data/res/strings/NOTES.STR`.

## Critical path

1. **Arrive at loc 22** via the real elevator MFD ("Recreational (5)") with the
   Operations campaign state preserved. Record which arrival email fires
   (Polito email 2 ⇒ `shodanroom` is unset; SHODAN email 9 or 11 ⇒ the
   transmitter briefing arrived and `note_5_7` is active).
2. Explore the crew annex / hotel levels. Take **log 1** (109) and **log 14**
   (108) — these activate `note_5_4` and are the in-fiction explanation of where
   the code comes from.
3. Optional but data-backed: read `rec3` log 8 for **11111**, come back and use
   `Keypad` **1955** to open the crew-annex level-2 stash and take the
   **`Viral Prolif`** (775). `note_5_8` completes here.
4. Optional: read log 11 (1983), then reach the pool-side breaker **807** and
   the `Light_button` **77** to complete `note_5_5`.
5. **REQUIRED — this is the gate to the whole south wing, not a side room.**
   Take **`Crew_Card2`** (80) in the athletic sector @ (73.70, 3.85, −120.58)
   (sets `ladycrewcard`), then frob `Card slot` **125** @ (−1.46, −3.40,
   −198.35) *while holding it* to open `Residential Door` **302** @
   (−0.95, −3.10, −196.60); `Experience Trap` 496 pays out.

   Verified 2026-07-25 by navmesh + in-game walk: door 302 is the **sole**
   entrance to everything south of z ≈ −201, including the transmitter. Every
   A* route from the north region to the south wing passes the same pinch
   (cells 3305 → 3306 → 3320). It is **not** reachable from rec3 (both rec1↔rec3
   return markers are at z = −98 and z = −187.4, north of the boundary) and not
   by an upper route. Without the card, `/v1/player/move` blocks at the door
   plane (−1.44, −4.19, −196.12).

   ⚠️ A prior session mistook this refusal for "an optional side room" and then
   spent most of the run proving a nonexistent traversal bug at z ≈ −201. **Do
   not re-probe that boundary — it is a locked door.** Confirmed walk route once
   open: (−0.83, −196.6) → (5.77, −199.8) → (3.5, −201) → (1.5, −206.7) →
   (−0.5, −211.9) → (−10.94, −214.33), standing under the transmitter.
6. **Work south to the transmitter.** Doors 1922/1923 (via `Simple Button` 124)
   are the authored gate; the real anchor is `Tripwire` **752** @
   (−8.47, −4.40, −137.87), which fires `Trigger Delay` 271 → button 124.
7. **Learn `14106` from the four `Code Art` frames** (see "Codes & required
   items"). ⚠️ **Blocked today by [#587](https://github.com/tommy-xr/shock2quest/issues/587)**
   — `PictureSwap` is a `NoopScript`, so the frames never cycle to their
   `code<N>` model. Until that lands, the code cannot be read in-world.
8. **Enter `14106` on `Transmitter Tower Off` 89** at (−11.31, 3.20, −214.75).
   The tower sits on a **mezzanine at y = +3.20**, ~7.4 units above the floor
   (y ≈ −5.2) — the climb is untested; check the `gravshaft 4` pair (778 / 416
   @ y = −0.30, z ≈ −251) at the far south end and the Base Rooms at y = 4.80.
   If the port cannot ride a grav shaft, that is the next real blocker.
   Verify: `Transmit` becomes raw 1, `note_5_4` and `note_5_7` complete, +20
   modules, the dish spins, and after 8 s the spawn ambush arrives.
9. Step ~960 frames and record which SHODAN email lands (6 "The cancer" +
   `note_5_3`, or 10 "My revenge" + `note_5_2`).
10. If (and only if) `reprogram` is also set: frob **`Big_Orange_Button` 653**,
   confirm doors 650/651 (then 497/652) open, enter the car, and frob
   **`Level Change Button` 664** → **`command1`, loc 56**. Otherwise the
   correct outcome is a `Message Trap` refusal and a return to Operations.

## Acceptance gate for Recreation

Mark Recreation **PASS** only if one reviewed session (or a frontier-backed
sequence) shows all of:

1. Real elevator arrival into `rec1` loc 22 with Operations state preserved.
2. The arrival email/objective wiring observed (`note_4_4` completes;
   `note_5_7` becomes active).
3. Logs 109 and/or 108 looted and read, with `note_5_4` observed active.
4. Physical traversal south through the 1922/1923 door pair under normal
   movement — not a teleport past it.
5. **`14106` typed into the real keypad on `Transmitter Tower Off` 89**, with
   `Transmit` raw 1, `note_5_4` + `note_5_7` complete, and the XP award.
6. The delayed consequence recorded after ≥ 960 frames: the correct SHODAN
   email and the correct follow-up objective for the current `reprogram` state.
7. `Big_Orange_Button` 653 demonstrated **refusing** while a gate bit is unset
   and **opening** once both are set (a frontier-resume with `reprogram`
   already set is acceptable for the second half).
8. Screenshots and `data.json` steps for each interaction and consequence.
   Quest-state assertions alone are not proof.

Specifically **insufficient**: visiting all three rec maps; hacking the
transmitter (it is authored un-hackable); setting `Transmit` through the debug
API; frobbing the Command elevator button without the door gate; or
direct-transitioning to `command1`.

## Known hazards on `rec1`

- **Cyborg Assassins 1374 / 1379** @ (−3.45, −3.14, 3.86) and
  (−3.24, −3.21, −0.29) — the north end of the map.
- **Droids**: `Assault` **487** @ (54.80, −3.91, −114.37), `Security` **740** @
  (51.72, −3.91, −103.48).
- **Red Monkeys** 1401 / 1671 (@ −14.40, −4.39, −76.76 and elsewhere), with
  `pyromonkey` set pieces 1464–1469.
- Heavy `Floor Egg Tripwire` (tmpl −1651) use with `Swarmer Floor Pod` and
  `Grub Floor Pod` clusters — Murdoch's log 4 is the in-fiction warning.
- **Security cameras** 144 / 159 / 175 / 181 / 322 and `Security Comp` 174 /
  180 / 338 / 347. Camera alert → security level is live on this deck.
- The **transmitter ambush**: two `DirectMonsterGen` spawns 8 s after activation.
- Scripted survivor set piece: `Cortez Trap` **95** @ (−1.47, −3.51, −128.99)
  (a `TrapNewTripwire` right by the elevator) → `Cortez signal trap` **96**
  (`PropSignalType("Cortez Signal")`) → AI **`MaleRec` 99** @
  (11.43, −3.62, −125.02), with marker `cortez` 103. Exactly what this NPC does
  was **not traced**; expect a scripted crew-member moment near the arrival hall.

## Engine watch-points / likely feature-gap boundaries

1. **`TrapUnlock` is a no-op in this port** (`shock2vr/src/scripts/mod.rs`:
   `"trapunlock" => Box::new(NoopScript::new())`). `Unlock Trap` **155** can
   therefore never unlock `Simple Button` **124**. Two consequences:
   - Frobbing button 124 directly will always refuse (`is_entity_locked`
     returns true for `PropLocked(true)` with no `PropKeyDst`).
   - `BaseButton`'s `TurnOn` path does **not** check the lock, so
     `Tripwire` 752 → `Trigger Delay` 271 → 124 still opens doors 1922/1923 —
     **even when `shodanroom` is unset**. The route is passable, but the
     authored "not until the SHODAN reveal" gate is not being enforced. Report
     as a fidelity gap, not a blocker.

   **CONFIRMED in play** (playtest 2026-07-25): with `shodanroom` set, frobbing
   button 124 does nothing and doors 1922/1923 stay shut; the tripwire chain
   opens them anyway, and appears to be one-shot (re-entry did not re-open).
   A diagnostic `TurnOn` moved both doors and their colliders correctly
   (1922: z −138.92→−141.24; 1923: −136.53→−134.21), so the doors themselves
   are fine — only the unlock path is missing.
2. **`TrapSpawn` is a no-op** (`"trapspawn" => NoopScript`). The two
   `DirectMonsterGen` objects fired 8 s after the transmitter will produce **no
   enemies**. Yang's security-alert ambush is silently missing; do not report
   "no ambush" as a data problem.
3. **`TrapMessage` is a no-op** (`"trapmessage" => NoopScript`). The three
   Command-elevator refusal messages (756 / 780 / 784) will not display, so a
   blocked player gets *silence*. Verify the gate by checking that the doors do
   not open, not by looking for a message.
4. **Art terminals are brush/texture content, not entities.** No entity in
   `rec1..rec3` displays a transmitter-code fragment (`rec3` object 179 is only
   an `Artechnology Sign` prop), and Yang's log 14 states the code was split
   across the deck's *art terminals* — so the digits live in texture frames.

   **Texture cycling itself is VERIFIED WORKING** (playtest 2026-07-25): a wall
   display in the rec1 arrival hall advances one frame per 12 sim-frames
   (200 ms), matching the `AnimatingScreenTex` metaprop (obj −2235) rate that
   the `tech/P00xx` families inherit. Property inheritance carries `P$AnimTex`
   to brush geometry, so an archetype lacking it directly still animates. Do
   **not** file a "texture animation is unimplemented" bug.

   **Still unresolved:** *which* texture family carries the 14106 digits. A
   non-exhaustive sweep of `tech/P00xx`, `Res_1/2/3`, `Res_Mark`, `Res_Mar2`,
   obj.crf `ART_*`, and a grep of `res/strings` + `strings.crf` did not find
   them (`Res_2/MOVA`+`MOVB` are the cinema "please stand by" 1‑2‑3‑4 leader,
   not the code). Not all 45 families or obj.crf model textures were decoded.
   Reading 14106 out of the mission data to unblock a run must be recorded as a
   shortcut, not as play.
5. **`ViralModify` and `ResearchableUseScript` are `UnimplementedScript`.** The
   `Viral Prolif` (775) can be picked up but its modify/research behaviour is
   absent.
6. **`KeypadUnhackable` maps to the ordinary `KeyPadGui`** — but the transmitter
   is **NOT hackable, correctly** (verified 2026-07-25): `keypad.rs:207`
   `hack_diff_for_entity()` returns `None` whenever the entity has a
   `PropKeypadCode`, so a keypad carrying a code never offers a hack affordance
   at all. `PropHackDiff { success_chance: −2000 }` is moot. Matches authored
   intent; not a bug. The code genuinely must be entered.
7. **`ElevState` is not Recreation evidence.** The port's `is_floor_available`
   lets deck 5 be selected from an unset state, so a fresh launch into `rec1`
   is useful for investigation but is **not** an end-to-end acceptance start.
8. `Regen_Hologram` **754** is present — bio-reconstruction/death-respawn is in
   play on this deck.
9. Trainers present in `rec1`: `Stats Trainer` 222, `Tech Trainer` 803,
   `Weapon Trainer` 804, `Psi Trainer` 965 (all map to `TrainerGui`), plus
   `RepBase` **425** / `RepScreen` **426** @ (−5.36, −1.59, −177.82) — worth
   exercising the recently-landed replicator hacking / nanite economy.

## What a real player does here (recalled from public walkthroughs — treat as unverified unless marked)

- **Verified from data**: the deck's objective is the transmitter; the code is
  split across art displays; the crew-annex stash needs 11111 and holds an
  exotic weapon; the pool-side breaker restores the basketball-court lights; the
  Command elevator needs both the transmitter and the Ops Sim Units.
- **Recalled, not verified here**: the usual retail route is elevator →
  crew/hotel section → athletic sector → transmitter, folding the garden
  (`rec2`) and mall (`rec3`) in as side trips for the codes, the Rec Crew card
  and upgrade units; the exotic weapon behind 11111 is the **Viral
  Proliferator** (this one *is* corroborated locally — object 775 is
  `sym:Viral Prolif`, tmpl −29); and the mall is described as the most
  dangerous part of the deck (Cortez log 5).
- **Do not assume** any particular door/window shortcut: no breakable-pane
  entity was identified on the transmitter route. Use door 1922/1923 and
  tripwire 752 as the anchors.

## Open questions

- **Where is `12345` taught?** `Keypad` **270** → `Floor Hatch` **626** uses it,
  but no deck-5 log in `LEVEL05.STR` contains that string. Possibly a
  designer-default code on an unimportant hatch.
- **`note_5_1`** has no string in `NOTES.STR` and no setter was found.
- **Chemical Manifest (deck 5 / log 32)** exists in `LEVEL05.STR` but no
  matching `PropLog` entity was found in `rec1..rec3`; it may be contained
  inside a searchable container that the flat entity dump did not surface.
- **Who/what is `MaleRec` 99 (the "Cortez" set piece)?** The signal type and
  marker are present; the AI's authored behaviour was not traced.
- **Does `Aux Power W/Out Battery` 807 require a battery item in this port?**
  Not determined.
- **`Note_4_3` / `Note_5_3` "Go to the Command Deck"** — `note_5_3` is set here
  by `EmailTrap` 120; `Note_4_3` still has no located setter (see `ops.md`).

## Sources

- Mission wiring: `cargo dq entities rec1.mis …` (2026-07-25), especially
  objects 26, 27, 29, 74, 77, 80, 89, 95, 96, 99, 108, 109, 111, 118–120, 124,
  125, 155, 158, 161, 162, 166–168, 170–173, 240, 244, 252, 270, 273, 274, 277,
  279, 293, 295, 302, 307, 481–483, 490, 495, 512, 543, 546, 555, 626, 653, 655,
  664, 668, 752, 775, 777, 778, 780, 782–784, 807, 890–893, 921, 1544, 1922,
  1923, 1955, 1983, 2014, 2016, and their SwitchLinks / `Contains` links.
- Retail text: `Data/res/strings/LEVEL05.STR` (deck-5 SHODAN emails 2/4/5/6/9/
  10/11/12 and audio logs 1–20, 32, including 11111 and 34093) and
  `Data/res/strings/NOTES.STR` (`Note_5_2`…`Note_5_8`).
- Port behavior: `shock2vr/src/scripts/gui/elevator.rs` (`ELEVATOR_STOPS`,
  `is_floor_available`), `shock2vr/src/scripts/mod.rs` (script registry —
  `trapunlock`, `trapspawn`, `trapmessage`, `viralmodify`,
  `researchableusescript`), `shock2vr/src/scripts/base_button.rs` and
  `shock2vr/src/scripts/script_util.rs` (`is_entity_locked`).
- `references/entities.md` for property/link semantics.
