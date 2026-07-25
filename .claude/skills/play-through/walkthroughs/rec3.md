# Walkthrough — rec3 (Recreation sector C: the mall, casino, theater, sim booths and security station)

Context for the `playtest` agent and the `play-through` reviewer. `rec3` is the
**third Recreation map** and, like `rec2`, is a side sector: it contributes one
side objective and a large amount of upgrade/loot content, but **no plot
progression**. The Recreation deck's objective — the transmitter — lives
entirely in `rec1` (see `rec1.md`). Nothing in `rec3` sets `Transmit`.

What `rec3` genuinely contributes:

1. **audio log 8** (Rosenberg), which teaches the crew-annex code **`11111`** and
   activates `note_5_8` — the objective completed back in `rec1`,
2. the deck's densest cluster of **upgrade units** (Stats / Tech / Weapon / Psi
   trainers) and replicators,
3. a world-placed **Crystal Shard**, several logs, and the Rec security station.

**Positions and positive IDs below are mission-file object identities** from
`cargo dq entities rec3.mis …`. Runtime entity IDs are assigned per launch and
are **not stable** — resolve by name and `template_id` each run.

## Entry and exits

`rec3` has **no main-elevator stop**; it is only reachable from `rec1` or `rec2`.

| From | World interaction | Position | Destination |
| --- | --- | --- | --- |
| `rec1` (east bulkhead) | `Bulk_On_Button` **274** | (74.35, −3.60, −98.11) | `rec3`, loc **500** → marker **135** @ (65.56, −3.60, −98.00) |
| `rec3` → `rec1` | `Bulk_On_Button` **381** | (60.24, −3.60, −98.01) | `rec1`, loc **500** |
| `rec1` (south bulkhead) | `Bulk_On_Button` **543** | (28.18, −3.60, −187.41) | `rec3`, loc **100** → marker **127** @ (29.75, −3.60, −187.80) |
| `rec3` → `rec1` | `Bulk_On_Button` **387** | (24.43, −3.60, −187.80) | `rec1`, loc **100** |
| `rec2` → `rec3` | `Bulk_On_Button` **540** | (102.70, −3.60, −95.05) | `rec3`, loc **400** → marker **140** @ (103.30, −3.60, −86.25) |
| `rec3` → `rec2` | `Bulk_On_Button` **582** | (103.30, −3.60, −80.94) | `rec2`, loc **400** |

Sector rooms named in the data (automap objects, template `1013`):
`0 Intro Mall`, `3 Upper Mall`, `4 Lower Mall`, `1 Up Cathouse`, `6 Down Cat`,
`5 Security`, `6 Theater`, `7 Casino`.

## The one objective that matters here

| Object | Position | Effect |
| --- | --- | --- |
| **Audio log 672** (`PropLog { deck: 5, log: 8 }`, Rosenberg "Looking out for #1") | **(78.54, −3.75, −204.44)** | Carries `PropQuestBitName("note_5_8")` = INCOMPLETE — activates *"An exotic weapon is on level 2 of the crew annex, code 11111."* Text: *"I stashed the thing on the second floor of the crew annex and jury rigged the door lock, code of 11111."* |

`note_5_8` is **completed in `rec1`**, by `Keypad` **1955** (`PropKeypadCode(11111)`)
→ `QB Set` **490**, which opens the door to the **`Viral Prolif`** (`rec1` object
775). So the honest order is: read log 672 here, then go back to `rec1`.

**No `TrapQBSet` in `rec3` writes any quest bit.** The only quest-bit-bearing
object in the whole mission is log 672.

## Cards and gated doors

| Gate | Position | Notes |
| --- | --- | --- |
| `Card slot` **295** | (77.82, −3.10, −98.16) | `PropKeyDst` region **32** (the **Rec Crew Access Card**, `rec2` object 996), `PropLocked(true)` → `Sci Med Door`s **2006** / **2008** and `OnFilter` **333** |
| `Card slot` **469** | (76.86, −3.10, −98.13) | Same region/target pair |
| `OnFilter` **333** | (73.81, −4.40, −100.15) | → `Unlock Trap` **492** @ (76.19, −4.40, −100.42), which unlocks both card slots |
| `Tripwire` **2011** | (77.51, −4.40, −96.12) | Also drives both card slots directly |

The sim-booth doors upstairs in the cathouse are `Button #1` **304 / 309 / 310 /
311** @ (73.9–74.0, 3.30, −198.7 / −208.8 / −211.2 / −214.8), each
`PropLocked(true)` with **no `PropKeyDst`**, each paired with an `Unlock Trap`
(305–308) *and* a `Tripwire` (1440 / 1442 / 1444 / 1445 @ y = 1.9) that also
drives the button directly. They open `Sci Med Door` **1436–1439**. The
`Sim-Love Broken` props (**204** @ (80.06, 3.32, −205.82), **214** @
(74.75, 3.13, −218.70), **237** @ (79.30, 3.32, −196.23)) are the sim units
themselves — broken, decorative.

## Items and upgrade units

| Object | Position | Notes |
| --- | --- | --- |
| **`Crystal Shard` 1941** | (82.16, −5.10, −177.62) | World-placed. `New Tripwire` **513** @ (84.10, −4.40, −176.93) → `EmailTrap` **514** (`PropLog { deck: 4, email: 6 }` — SHODAN's "An elegant weapon" shard briefing) |
| `Psi Trainer` **114** | (83.37, −3.40, −184.45) | `PsiTrainer` → `TrainerGui` |
| `Weapon Trainer` **116** | (82.86, −3.40, −188.66) | |
| `Tech Trainer` **111** / `Stats Trainer` **112** | (83.47 / 92.58, −3.60, −123.04) | The upgrade lounge — `Neural Implants Sign` **154** @ (103.16, −1.70, −132.23) marks it |
| `Stats Trainer` **172** / `Tech Trainer` **173** | (107.60 / 110.55, −3.40, −135.06) | Second cluster |
| `RepBase` **232** / **240** / **757** / **1139** | (89.40, −1.72, −153.22), (72.55, 4.68, −192.89), (89.06, 3.93, −113.77), (107.91, 4.11, −133.66) | Four replicators — the densest set on the deck. Rosenberg's log 20 is about them |
| `Regen_Hologram` **496** | — | `Teleport Trap` **497** @ (84.13, −3.20, −168.24) moves it into place |
| `Psi Booster` 233, 234; hackable crates 178, 181, 183; five `Closed Protocol Box`es 1098–1102 | — | Loot |

## Audio logs in `rec3` (`PropLog { deck: 5, log: N }`)

| Log | Speaker / subject | Object | Position |
| --- | --- | --- | --- |
| **8** | **Rosenberg, "Looking out for #1" — code 11111** | **672** | (78.54, −3.75, −204.44); carries `note_5_8` |
| 10 | Siddons, "Find me" (bitten by a spider) | 1980 | (75.39, −3.20, −173.09) |
| 18 | Korenchkin, "Coming home" | 87 | (46.84, −2.57, −229.82) |
| 19 | Rosenberg, "My nanites" (left in the Sensual Sim center) | 86 | (108.49, −3.68, −148.92) |
| 20 | Rosenberg, "Defending the reps" (his death log, at a replicator) | 198 | (91.59, 0.45, −139.58) |

## Recommended route (no critical path exists here)

1. Enter from `rec1` (274 → loc 500, or 543 → loc 100) or from `rec2` (540 →
   loc 400).
2. Sweep the mall and casino; Cortez's `rec1` log 5 (*"Stay out of the mall if
   you can. It crawls."*) is the in-fiction warning for this map.
3. Take **log 672** in the cathouse/sim area so `note_5_8` is active.
4. Work the upgrade lounges (trainers 111/112/116/114/172/173) and the four
   replicators.
5. Take the **Crystal Shard** (1941) and observe SHODAN's shard email.
6. Use the **Rec Crew Access Card** at slot 295 or 469 if the security-station
   route is wanted.
7. Return to `rec1` and use **11111** on `Keypad` 1955 to complete `note_5_8` and
   take the `Viral Prolif`.

## Known hazards on `rec3`

- **`Laser Turret` 1726** @ (71.06, −4.06, −180.97).
- **Droids**: `Security` **839** @ (101.70, −3.91, −93.57), `Security` **1027** @
  (98.98, −3.91, −166.75), `Assault` **1047** @ (92.63, −3.91, −146.01).
- Three `Protocol Droid`s (141, 235, 266).
- Heavy egg seeding: ~11 `Floor Egg Tripwire`s with `Swarmer Floor Pod` and
  `Grub Floor Pod` clusters throughout the mall.
- **`WormMind` 832** — an annelid psi-mind set piece.
- **Security cameras** 83, 90, 103, 123, 161 with `Security Comp` 190, 194, 205.
  The `5 Security` room is the Rec security station.
- A one-shot set piece in the theater/lower area: `New Tripwire` **526** @
  (47.44, −9.99, −161.76) and **527** @ (53.24, −11.10, −157.95) both fire
  `Tweq Trap` **525** @ (47.62, −11.49, −158.42) and `Trigger Delay` **529**,
  which fires `Destroy Trap` **528** to remove both tripwires. Exactly what the
  tweq animates was **not traced**.
- `Arachnid Corpse` 498 — Siddons' log 10 is the matching story beat.

## Engine watch-points

1. **`TrapUnlock` is a no-op** in this port. The four locked sim-booth buttons
   (304 / 309 / 310 / 311) and the two card slots (295 / 469) can never be
   unlocked by their `Unlock Trap`s. Practically:
   - the card slots still work, because they carry `PropKeyDst` region 32 and
     `is_entity_locked` clears once the player holds the Rec Crew card;
   - the sim-booth buttons will **refuse a direct frob forever** (locked, no
     `PropKeyDst`), but their co-located tripwires (1440–1445) drive the doors
     through `BaseButton`'s unchecked `TurnOn` path. Expect "walking up opens
     it, pressing it does nothing" — a fidelity gap, not a blocker.
2. **`TrapSpawn` / `TrapMessage` are no-ops**; `TrapTeleport`, `TrapDestroyer`,
   `TrapDelay`, `TrapSound`, `TrapEXPOnce`, `TrapEmail`, `TrapQBSet`,
   `TrapQBFilter`, `TrapQBNegFilter`, `TrapQuestbitSimple`, `PsiTrainer` and
   `TrapRouter` **are** implemented.
3. Log **672** carries its objective as `PropQuestBitName` **directly on the
   disc**, with no SwitchLink fallback — the same pattern as the Ops logs
   covered by issue **#568** (log discs' quest-bit metadata ignored by the
   current `MediaGui` / `CollectLog` path). If reading it does not activate
   `note_5_8`, that is the known activation bug; do not inject the bit.
4. `rec3` has four replicators — the best place on the deck to exercise the
   recently-landed replicator hacking and nanite economy (#545, #546).

## What a real player does here (recalled — treat as unverified)

Public walkthroughs describe the Recreation mall/casino sector as the deck's
optional but heavily-stocked area: the shopping mall, the casino, the theater,
the "sensual sim" booths, and a bank of upgrade units, with Rosenberg's log
pointing back to the crew annex stash. The transmitter is **not** here, and no
retail account places a plot gate in this sector. All of that matches the data.
The specific room-by-room route, and any claim about what is inside individual
sim booths, is **recalled/unverified** — navigate by the anchors above.

## Open questions

- What does the `Tweq Trap` **525** set piece (fired by tripwires 526 / 527)
  actually animate? Not traced.
- Deck-5 **log 32 (Chemical Manifest: Rec)** exists in `LEVEL05.STR` but no
  matching `PropLog` entity was found in `rec1..rec3`.
- Nothing in `rec3` was found that displays a **transmitter code fragment**; the
  only art-related entity is `Artechnology Sign` **179** @ (101.53, 3.80,
  −147.01), which is a plain prop. The code fragments therefore appear to be
  brush/texture content — see `rec1.md` watch-point 4.

## Sources

- Mission wiring: `cargo dq entities rec3.mis …` (2026-07-25), especially
  objects 86, 87, 111, 112, 114, 116, 127, 135, 140, 141, 154, 172, 173, 179,
  190, 194, 198, 204, 205, 214, 232, 235, 237, 240, 266, 274, 295, 304–311, 333,
  381, 387, 469, 492, 496, 497, 513, 514, 525–529, 582, 672, 757, 832, 839,
  1027, 1047, 1139, 1436–1439, 1440–1445, 1726, 1941, 1980, 2006, 2008, 2011.
- Retail text: `Data/res/strings/LEVEL05.STR` (logs 8, 10, 18, 19, 20),
  `Data/res/strings/NOTES.STR` (`Note_5_8`).
- Port behavior: `shock2vr/src/scripts/mod.rs` (script registry),
  `shock2vr/src/scripts/base_button.rs`, `shock2vr/src/scripts/script_util.rs`
  (`is_entity_locked`).
- Companion notes: `rec1.md` (the deck's actual objective and the Command
  elevator), `rec2.md`.
