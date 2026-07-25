# Walkthrough — rec2 (Recreation sector B: the garden, the mess/lounges, and the Rec Crew card)

Context for the `playtest` agent and the `play-through` reviewer. `rec2` is a
**side sector of the Recreation deck**, not its objective. The deck's plot
objective — the transmitter — lives entirely in `rec1` (see `rec1.md`). What
`rec2` genuinely contributes is:

1. the **Rec Crew Access Card**, which opens region-32 doors in *all three* Rec
   maps,
2. the **garden maintenance tunnel** side objective (`note_5_6`, code `34093`),
3. Cortez/Delacroix/Siddons/Suarez audio logs, a Psi Amp, a Crystal Shard, and a
   Grenade Launcher.

Nothing in `rec2` sets `Transmit` or touches the Command elevator.

**Positions and positive IDs below are mission-file object identities** from
`cargo dq entities rec2.mis …`. Runtime entity IDs are assigned per launch and
are **not stable** — resolve by name and `template_id` each run.

## Entry and exits

`rec2` has **no main-elevator stop and no level-start marker for deck arrival**;
it is only reachable from `rec1` or `rec3`.

| From | World interaction | Position | Destination |
| --- | --- | --- | --- |
| `rec1` (west bulkhead) | `Bulk_On_Button` **546** | (−12.50, −3.60, −66.32) | `rec2`, loc **200** → marker **270** @ (−12.50, −3.60, −75.95) |
| `rec2` → `rec1` | `Bulk_On_Button` **273** | (−12.51, −3.60, −81.25) | `rec1`, loc **200** |
| `rec1` (east bulkhead) | `Bulk_On_Button` **555** | (27.41, −3.60, −74.24) | `rec2`, loc **300** → marker **264** @ (27.40, −3.60, −81.93) |
| `rec2` → `rec1` | `Bulk_On_Button` **507** | (27.40, −3.60, −87.27) | `rec1`, loc **300** |
| `rec2` → **`rec3`** | `Bulk_On_Button` **540** | (102.70, −3.60, −95.05) | `rec3`, loc **400** |
| `rec3` → `rec2` | `Bulk_On_Button` **582** | (103.30, −3.60, −80.94) | `rec2`, loc **400** → marker **242** @ (102.68, −3.60, −89.72) |

Sector rooms named in the data (automap objects, template `1013`): `0 Garden`,
`1 Lower garden`, `2 low dining`, `3 tunnels`, `4 Rec2`, `5 Lounge`,
`6 Near Mess`, `7 Kitchen`, `7 Dining`, `8 UpperLounge`, `9 Upper bar`,
`Bar #2`.

## Codes, cards, and gated doors

| Item / code | Genuine source (data-verified) | What it opens |
| --- | --- | --- |
| **`Rec Crew Key` — "Rec Crew Access Card"** (object **996**, tmpl −1370, `PropKeySrc` region **32**) | World-placed @ **(98.00, −7.94, −52.83)**. `PropScripts { ["FrobQB"] }` sets quest bit **`crewcahd`** on pickup; `Simple QB Trigger` **721** @ (97.77, −2.63, −57.75) then pays `Experience Trap` **714** | Every region-32 card slot on the deck: `rec2` **623** @ (31.55, −3.71, −69.96) and **1004** @ (31.22, −3.71, −67.46) → `Sci Med Door`s **1458** / **1460**; `rec1` 293 / 2016 (and master slots 295 / 2014); `rec3` 295 / 469 |
| **`34093`** (garden maintenance tunnel) | Audio log **528** (deck 5 / log 16, **Cortez, "Under the garden"**) @ (10.05, −8.77, −2.57), carrying `PropQuestBitName("note_5_6")` = INCOMPLETE. *"I've chosen the maintenance tunnel underneath the garden as an internment site, keypad code 34093."* | `Keypad` **122** (`PropKeypadCode(34093)`) @ **(94.25, −8.00, −86.79)** → `Sci Med Door` **1475** @ (93.10, −7.90, −87.54), **`QB Set` 503** (`note_5_6` = COMPLETE), **and two `DirectMonsterGen`** (1301 @ (87.29, −2.51, −50.25), 1321 @ (89.79, −2.51, −50.37)) — Cortez's *"I don't trust the dead"* is not a metaphor |
| **`50220`** | No teaching log found | `Keypad` **624** @ (−0.45, −3.09, −63.65) → `LockedDoor` **1390** @ (−2.50, −3.08, −62.81). **Carries `PropObjState(Broken)`** — authored as a broken keypad; `Button #1` **1391** @ (−0.27, −3.09, −61.60) drives the same door |

`Card slot` **241** @ (47.98, −4.30, −23.95) → `Residential Door` **245** @
(56.64, −3.10, −14.52) is not region-keyed; it is driven by `Tripwire` **246**
and by `Unlock Trap` **148**, which is fired by `Simple Button` **146** @
(43.66, −7.70, −16.77) (a lights + unlock two-state button).

## Quest bits touched here

| Bit | Set by | Meaning |
| --- | --- | --- |
| `note_5_6` | activated by log **528**, completed by `QB Set` **503** @ (93.47, −9.20, −85.02) ← `Keypad` **122** | "The code for the garden maintenance tunnel is 34093." |
| `crewcahd` | `Rec Crew Key` **996** (`FrobQB`) | First-pickup XP marker for the Rec Crew card |

**No other quest bit is written by `rec2`.** In particular it does not set
`Transmit`, `reprogram`, or any `Note_5_2/3/4/5/7/8`. If a session reports
Recreation progress from `rec2` alone, that is a review failure.

## Items worth taking

| Object | Position | Notes |
| --- | --- | --- |
| **`Psi Amp` 525** | (8.65, −4.67, −11.93) | **Inside `Male Corpse 2` 484** @ (9.28, −5.07, −11.09) — must be looted through the container MFD |
| **`Crystal Shard` 590** | (8.76, −4.67, −10.92) | Also inside corpse 484. `New Tripwire` **745** @ (12.98, −4.67, −12.69) → `EmailTrap` **746** (`PropLog { deck: 4, email: 6 }` — SHODAN's "An elegant weapon" shard briefing) |
| **`Gren Launcher` 479** | (102.47, −9.41, −80.41) | Down in the garden maintenance tunnel area |
| `Hack Soft V3` 201 | — | Hacking software |
| `RepBase` **487** / `RepScreen` **492** | (−0.79, −6.44, −5.33) | Replicator — worth exercising hacking / nanite economy |
| `Regen_Hologram` 356 | — | Bio-reconstruction |

Two `Psi Booster`s (257, 258), four `Closed Protocol Box`es (1295, 1296, 1298,
1300) and several hackable crates (195, 199, 526) are also present.

## Critical path (there isn't one — this is the recommended side route)

1. Enter from `rec1` through either bulkhead (546 → loc 200, or 555 → loc 300).
2. Sweep the mess / lounge / bar block for logs 12 (1982), 9 (1979), 13 (1984)
   and 17 (76).
3. Reach the garden and take **log 16** (528) so `note_5_6` is active *before*
   the keypad is used.
4. Cross the garden east and take the **Rec Crew Access Card** (996) @
   (98.00, −7.94, −52.83). Expect the ambush (below).
5. Use `Keypad` **122** with **34093** to open the maintenance tunnel; confirm
   `note_5_6` completes and handle the two spawns.
6. Loot the Psi Amp / Crystal Shard from corpse 484 through the container MFD,
   and pick up the Grenade Launcher (479).
7. Return to `rec1` (or continue to `rec3` via 540) with the card.

## Audio logs in `rec2` (`PropLog { deck: 5, log: N }`)

| Log | Speaker / subject | Object | Position |
| --- | --- | --- | --- |
| 6 | Cortez, "Worm artifact" (Taylor's artifact) | 682 | (98.65, −9.37, −64.85) |
| 9 | Suarez, "Where are you?" (helping set up the transmitter) | 1979 | (92.68, −4.24, −40.71) |
| 12 | Siddons, "Escape pods" | 1982 | (67.84, −3.53, −40.42) |
| 13 | Murdoch, "-------" (his death log) | 1984 | (−14.62, −3.46, −43.02) |
| **16** | **Cortez, "Under the garden" — code 34093** | **528** | (10.05, −8.77, −2.57); carries `note_5_6` |
| 17 | Delacroix, "Trusting SHODAN" (transmitter + ops computers) | 76 | (8.75, −8.17, −41.88) |

## Known hazards on `rec2`

- **The Rec Crew card ambush.** `Tripwire` **187** @ (93.82, −9.20, −54.04)
  (`ENTER | ONCE | PLAYER`) fires, in one shot: `Sci Med Door` **1476**,
  three `Teleport Trap`s **176 / 179 / 185** (176 teleports an **`OG-Grenade`**
  into position — a proximity grenade trap), `AI Signal Trap` **171**, and
  `Destroy Trap` **140** (destroys `Grate` 345). Approach the card expecting it.
- **The corridor ambush between the bulkheads and the garden.** `New Tripwire`
  **373** @ (40.92, −4.40, −49.61) and **324** @ (35.29, −4.40, −57.06) both fire
  the same set: seven `Tweq Trap`s, `Junction Box` **368** @
  (36.29, −1.00, −52.75), `AI Signal Trap` **97** → **`Assault` droid 2143** @
  (33.76, −3.91, −57.95), and `Destroy Trap` **92**, which destroys *both*
  tripwires so the set piece is one-shot.
- **`Laser Turret` 1724** @ (−18.30, −4.06, −32.47), **`Security` droid 851** @
  (−14.76, −3.91, −28.28) — the west lounge approach.
- **Baby Arachnids** 169, 170; heavy `Floor Egg Tripwire` / `Swarmer Floor Pod`
  / `Grub Floor Pod` seeding across the garden.
- **Security cameras** 205, 210, 232, 237 with `Security Comp` 209, 236.
- Vertical traversal: `Lift 1` **2235** (grav lift) @ (56.32, −10.38, −19.67)
  with call buttons 2238 / 2240 @ (55.70, −8.15 / −3.12, −21.05); `Floor Hatch`
  **430** @ (8.32, −0.01, −61.10) driven by `Button #1` **74**.

## Engine watch-points

1. **`TrapSpawn` is a no-op** in this port, so the two `DirectMonsterGen`
   objects behind the 34093 keypad will spawn nothing. Do not report the missing
   "internment site" ambush as a data bug.
2. **`TrapUnlock` is a no-op**, so `Unlock Trap` **148** cannot unlock `Card
   slot` **241**; the slot is also driven directly by `Tripwire` 246, which
   should still work through `BaseButton`'s unchecked `TurnOn` path.
3. **`TrapTeleport` *is* implemented**, so the OG-Grenade trap at the Rec Crew
   card should fire; if it does not, that is a real finding.
4. `Keypad` **624** ships `PropObjState(Broken)`. Whether the port honours a
   broken keypad (refusing the code) was **not verified** — check before
   treating a refusal there as a bug.
5. Container loot (Psi Amp 525, Crystal Shard 590 in corpse 484) must go through
   the container MFD. **Never** `/v1/player/give` a container-held item — it
   permanently severs the `Contains` link (issue #572).

## What a real player does here (recalled — treat as unverified)

Public walkthroughs describe the Recreation garden/mess sector as an optional
loot-and-logs detour off the crew section, where you pick up the deck's crew
access card, hear Cortez's warning about the bodies under the garden, and open
the maintenance tunnel with 34093 for the stash below. The transmitter is *not*
here. The data agrees with all of that; the specific room-by-room route is
**recalled, not verified** — navigate by the anchors above.

## Open questions

- **Where is `50220` taught?** No deck-5 log contains it. The keypad is marked
  `Broken`, so it may be intentionally unusable with a button (1391) as the real
  opener.
- Deck-5 **log 32 (Chemical Manifest: Rec)** exists in `LEVEL05.STR` but no
  matching `PropLog` entity was found in `rec1..rec3`.
- The `gardentrap` (104), `gardenrun` (293), `GardenEXP` (520) and `gardenmail1`
  (488) objects suggest a scripted garden set piece that was **not fully
  traced**.

## Sources

- Mission wiring: `cargo dq entities rec2.mis …` (2026-07-25), especially
  objects 74, 76, 92, 97, 122, 140, 146, 148, 176, 179, 180, 185, 187, 241, 242,
  245, 246, 264, 270, 273, 297, 324, 373, 430, 479, 484, 503, 507, 525, 528,
  540, 590, 623, 624, 682, 714, 721, 745, 746, 851, 996, 1004, 1024, 1301, 1321,
  1390, 1391, 1458, 1475, 1476, 1724, 1979, 1982, 1984, 2143, 2235, 2238, 2240.
- Retail text: `Data/res/strings/LEVEL05.STR` (logs 6, 9, 12, 13, 16, 17),
  `Data/res/strings/NOTES.STR` (`Note_5_6`).
- Port behavior: `shock2vr/src/scripts/mod.rs` (`trapspawn`, `trapunlock`,
  `trapteleport`), `shock2vr/src/scripts/script_util.rs`.
- Companion notes: `rec1.md` (the deck's actual objective), `rec3.md`.
