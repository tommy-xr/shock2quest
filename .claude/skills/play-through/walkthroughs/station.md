# Walkthrough — earth → station (recruitment and three training years)

Context for the `playtest` agent and the `play-through` reviewer. This is the
real character-generation path from the Ramsey Recruitment Center into the
Von Braun campaign. The player must choose a service on `earth.mis`, walk the
Wake Island / Station 74 training corridor three times, choose one of three
authored assignments each year, and deploy naturally to `medsci1.mis`.

This is not a coordinate script. Observe the signs, recruiter, year display,
and three assignment exhibits; walk through the chosen service door and
assignment lane. The coordinates and mission object IDs below are diagnostic
anchors for review, not permission to teleport onto the outcome trigger.

Positive IDs are stable mission-file object identities from `cargo dq`.
Runtime entity IDs change on every load and must be rediscovered by name or
`template_id`. Reloading `station.mis` after years 1 and 2 is an authored part
of this sequence, so rediscover all runtime entities after each reload.

## What this sequence must prove

- Earth career selection works through a physical door crossing.
- Exactly one of Marine, Navy, or OSA persists as the player's career.
- Station arrives at its authored recruitment-deck spawn and stages the
  matching service/year exhibit.
- Three distinct assignment choices are visible each year and the chosen lane
  grants the matching reward.
- Years advance monotonically: no bits → `training_year_2` → `_3` → `_4`.
- The first two assignments legitimately reload Station with the next year's
  authored content. The third deploys to MedSci instead of looping a fourth
  time.
- Career, loadout, accumulated stats/skills/psi disciplines, and inventory
  survive all transitions.

## Earth handoff — choose a service in-world

Finish or deliberately decline the optional Earth training rooms, then enter
one of the three career doors on foot. The career-door tripwire centers below
are useful for diagnosing the real `TrapNewTripwire → SwitchLink →
ChooseService` wiring, but teleporting onto one is only a focused regression
probe, not a genuine play-through.

| Service | Earth tripwire center | Career bit | Station loadout |
| --- | --- | --- | --- |
| Marines | (−2.499, 24.0, 68.163) | `career_marine` | 45 max HP, 20 max psi |
| Navy | (0.509, 24.0, 84.064) | `career_navy` | 35 max HP, 35 max psi |
| OSA | (16.858, 24.0, 80.747) | `career_osa` | 30 max HP, 60 max psi, extra Kinetic Redirection / `PsiPull` |

The Navy marker is mislabeled `SendToMarines` in the retail mission data, but
its `P$Service=1` correctly means Navy. Judge the persisted quest bit and
loadout, not that editor name.

On the transition to `station.mis`, require:

1. Exactly the selected `career_*` bit is `complete`; the other two remain
   `unknown`.
2. HP/psi match the table.
3. `player.stats.granted_years` is empty and no `training_year_*` bit is
   complete.
4. The player is near the authored `Starting_Location` object 132,
   (81.780, −3.600, 16.539), rather than the world origin.
5. The appropriate first-year START root has run:
   `START_01` Marines, `START_11` Navy, or `START_21` OSA.

Recruiter staging is **year-specific, not career-specific**. In year 1, object
881 (`MaleRec`) moves from below the map to about (15.6, −3.2, 10.4) for every
career. That proves the shared `YEAR-1` presentation ran, but not which career
root ran. Prove the career using the three matching assignment objects listed
below. The editor markers `GO-MARINES`, `GO-NAVY`, and `GO-OSA` are inert.

## Station layout and the honest route

All three years reuse the same long recruitment corridor:

1. Start on the recruit deck around x≈81.8.
2. Follow the only forward public route toward decreasing x. Pass the
   service-specific recruiter and year display. Do not bridge walls or jump to
   the far end.
   Exercise the real corridor doors: approach sensors 95 and 157 open doors
   156 and 158; initial debrief sensor 766 opens door 153; sensor 173 opens
   door 68.
3. At x≈0, stop and inspect all three service/year assignment exhibits. The
   START router moves the current three authored mission objects into these
   positions:

   | Tour index | Exhibit lane | Assignment trigger |
   | --- | --- | --- |
   | 0 | (0.012, −1.6, +8.0) | tripwire 741 at (−22.6, −5.6, +8.0) → marker 125 |
   | 1 | (0.012, −1.6, 0.0) | tripwire 124 at (−22.6, −5.6, 0.0) → marker 126 |
   | 2 | (0.012, −1.6, −8.0) | tripwire 745 at (−22.6, −5.6, −8.0) → marker 127 |

4. Choose based on the visible assignment/reward, then walk through that same
   lane. The final lane-door sensors 309/305/742 must first open doors
   310/306/743 for the +8/center/−8 lanes; continue on foot until the
   corresponding ENTER tripwire fires. The marker's `P$CharGenRo` value
   (0/1/2) is the selected tour.
5. After years 1 and 2, allow the normal `station.mis` transition to finish,
   confirm the next year is staged, and walk the corridor again. After year 3,
   require the real marker transition to `medsci1.mis`.

Do not rely on left/right labels, which depend on approach and camera
orientation. Match the visible assignment text to the +8, center, or −8 lane
when reviewing telemetry.

Use these stable mission-object IDs to prove that the correct three signs
replaced the preceding year's set:

| Career | Year 1 (+8 / 0 / −8) | Year 2 (+8 / 0 / −8) | Year 3 (+8 / 0 / −8) |
| --- | --- | --- | --- |
| Marines | 356 / 358 / 359 | 360 / 361 / 362 | 369 / 370 / 388 |
| Navy | 389 / 390 / 395 | 396 / 397 / 398 | 399 / 400 / 401 |
| OSA | 402 / 406 / 407 | 408 / 409 / 410 | 411 / 412 / 413 |

## The three-year choice matrix

`Mission1..27` in `Data/res/strings/CHARGEN.STR` are the retail assignment and
debrief texts. The current runtime stores the following grants in
`player.stats`. Tour 0/1/2 always corresponds to the +8/0/−8 lane above.

### Marines

| Year | Tour 0 / +8 | Tour 1 / center | Tour 2 / −8 |
| --- | --- | --- | --- |
| 1 (`START_01`) | Mission1, UNN Gallo: **+2 Strength** | Mission2, Io Survival School: **+2 Endurance** | Mission3, Guadacanal Station: **+2 Agility** |
| 2 (`START_02`) | Mission4, UNN Antigua: **+1 Energy Weapons, +1 Cyber Affinity** | Mission5, Asteroid Belt: **+1 Heavy Weapons, +1 Cyber Affinity** | Mission6, Port MacArthur: **+2 Standard Weapons** |
| 3 (`START_03`) | Mission7, UNN Home Office: **+1 Maintenance** | Mission8, Polidies Station: **+1 Modify** | Mission9, UNN Antigua: **+1 Repair** |

Staging anchors for the Marine tour-0 path used by the current end-to-end test:
after year 1, `Marines 4` object 360 must be at (0.012, −1.6, +8); after year
2, `Marines 7` object 369 must occupy the same exhibit lane. The corresponding
objects for the other lanes are equally valid choices.

### Navy

| Year | Tour 0 / +8 | Tour 1 / center | Tour 2 / −8 |
| --- | --- | --- | --- |
| 1 (`START_11`) | Mission10, UNN Lucille: **+1 Hack, +1 Strength** | Mission11, UNN Lucille: **+1 Repair, +1 Strength** | Mission12, UNN Lucille: **+1 Modify, +1 Strength** |
| 2 (`START_12`) | Mission13, UNN Carfax: **+2 Cyber Affinity** | Mission14, UNN Pierce: **+1 Maintenance** | Mission15, LaVerne Tactical School: **+2 Standard Weapons** |
| 3 (`START_13`) | Mission16, Marie Curie Research: **+1 Research** | Mission17, Io Survival Training: **+2 Endurance** | Mission18, Yamamoto Station: **+2 Agility** |

### OSA

| Year | Tour 0 / +8 | Tour 1 / center | Tour 2 / −8 |
| --- | --- | --- | --- |
| 1 (`START_21`) | Mission19, Shao Ling: **Cryokinesis + Psychogenic Cyber Affinity** | Mission20, Ru Nang: **Cryokinesis + Kinetic Redirection** | Mission21, Chu Lun: **Cryokinesis + Psycho-reflective Screen** |
| 2 (`START_22`) | Mission22, OSA Central Core: **+2 Psionic Ability** | Mission23, Ki Luan: **+1 Research** | Mission24, Io Facility: **+2 Endurance** |
| 3 (`START_23`) | Mission25: **+1 Strength, +1 Agility, +1 Cyber Affinity + Psychogenic Agility** | Mission26: same stats **+ Neuro-Reflex Dampening** | Mission27: same stats **+ Remote Electron Tampering** |

The retail OSA year-1 text also says Tier Two psi disciplines are unlocked. The
port currently records the named disciplines in `player.stats.psi_disciplines`
but does not yet wire all training grants into the active psi-power system.
Record that limitation separately; do not claim active-power behavior from the
stored list alone.

## Year-by-year state transitions

### Year 1

1. Confirm the service's year-1 START root, recruiter, `YEAR-1` display, and
   three year-1 assignments are visibly staged.
2. Record baseline `player.stats` before choosing.
3. Walk from the spawn through the corridor and enter one chosen assignment
   lane.
4. The chosen reward applies once, `granted_years` becomes `[1]`, and exactly
   `training_year_2` becomes complete.
5. A transition back to `station.mis` is expected. It must activate the
   selected service's year-2 root, not repeat year 1.

### Year 2

1. Confirm `START_02`, `START_12`, or `START_22`, the `YEAR-2` display, and
   three different year-2 assignments. A stale recruiter/year-1 set is a
   blocker even if the exit tripwires remain reachable.
2. Walk the corridor again and enter the intended lane.
3. Require the exact year-2 reward, `granted_years == [1, 2]`, and completed
   bits `training_year_2` plus `training_year_3`.
4. The second transition back to Station is also expected and must stage the
   service's year-3 root.

An optional persistence probe may save and reload here, then compare the whole
character sheet byte-for-byte. This is not part of the retail narrative path,
so label it as a save/load test rather than a fourth training transition.

### Year 3 and deployment

1. Confirm `START_03`, `START_13`, or `START_23`, the `YEAR-3` display, and
   the three authored final-year assignments. The robot/dance presentation in
   the retail sequence is useful visual evidence that year 3 staged correctly,
   but the assignment objects and reward are authoritative.
2. Walk the corridor a third time and enter the chosen lane.
3. Require the exact year-3 reward, `granted_years == [1, 2, 3]`, and completed
   bits `training_year_2`, `_3`, and `_4`. `training_year_1` is intentionally
   never authored; the counter begins at year 1 and writes the next year.
4. The same ChooseMission marker now follows its `PropDestLevel=MedSci1` and
   `PropDestLoc=2502`. It must load `medsci1.mis` naturally.
5. In MedSci, recheck the selected career bit, HP/psi loadout, full accumulated
   character sheet, and finite player position. Then continue with the
   MedSci walkthrough.

A third reload into Station, a fourth walk down the corridor, repeated
year-3 assignments, or rewards that apply twice indicates a progression loop
and is a campaign blocker.

## Genuine-play acceptance gate

Mark this segment **PASS** only when one reviewed session demonstrates:

1. A career door is approached and crossed on foot in Earth; no direct
   `TurnOn`, transition API, or career-bit mutation.
2. Station loads at object 132's authored spawn with exactly one career bit
   and the correct branch loadout.
3. The year-1 presentation stages its shared recruiter and year display, while
   the selected career's START root stages all three matching assignment
   exhibits.
4. For each of three years, the player physically traverses from the spawn
   area to x≈0, inspects the available assignments, and continues through the
   selected matching lane to x≈−22.6. Meaningful movement steps and screenshots
   must cover the corridor; a single player teleport does not.
5. The first and second choices produce exactly two legitimate Station
   reloads, with year-2 then year-3 content visibly replacing the prior year.
6. Each chosen reward matches its career/year/tour matrix entry and is applied
   only once. `granted_years` and `training_year_*` advance monotonically.
7. The third choice naturally loads MedSci1, where career, loadout, and all
   accumulated rewards persist.
8. `data.json` records screenshots before each lane choice, the physical entry,
   the resulting quest/stat delta, each reload's newly staged content, and the
   final MedSci frontier.

The campaign needs one coherent career path, not all three careers in a single
session. Branch-focused follow-ups should repeat the full Earth enlistment and
at least enough genuine Station traversal to validate Navy/OSA-specific
staging and rewards.

## What the current SDK tests do and do not prove

`station-flow.e2e.test.ts` is a valuable regression net: it teleports onto the
real Earth and Station tripwire volumes, thereby exercising genuine
`TrapNewTripwire → SwitchLink → ChooseService/ChooseMission` script wiring. Its
Marine tour-0 chain correctly checks:

- START staging;
- +2 Strength, then +1 Energy Weapons/+1 Cyber Affinity, then +1 Maintenance;
- `[1] → [1,2] → [1,2,3]` reward persistence;
- the two Station reloads, a mid-flow save/load, and final MedSci deployment;
- all three career doors in independent fresh launches.

It deliberately does **not** prove:

- normal navigation from Earth's lobby to a career door;
- the long Station corridor or collision/door traversal;
- inspection or selection of tour 1 and tour 2 lanes;
- a complete Navy or OSA three-year run;
- that visible recruiter, year, and assignment presentation is correct beyond
  the explicit staging anchors.

The sibling `station-career*.e2e.test.ts` tests are even narrower: direct
marker `TurnOn` or `transitionLevel()` calls validate state/persistence in
isolation, not the player's path. Use these tests to diagnose failures, never
as the acceptance evidence for this walkthrough.

## Engine watch-points

- A tripwire teleport can fire the same ENTER sensor as walking, so quest bits
  alone cannot distinguish a shallow test from genuine traversal. Review the
  movement history and screenshots.
- Runtime entity IDs are invalid after every Station reload. Keeping one ID
  across years can silently act on a different object.
- The three assignment triggers are always physically present. Therefore an
  advancing year bit does not prove the corresponding START router staged the
  correct visible assignments.
- Career HP/psi are reapplied from the persisted career bit on mission load.
  Verify both the bit and the values; either one alone is incomplete evidence.
- OSA's career loadout supplies active Kinetic Redirection, while OSA tour
  disciplines are currently character-sheet storage. Keep those two mechanisms
  distinct in findings.
- Station is intentionally low-combat. Its meaningful mechanics are traversal,
  trigger wiring, scripted staging, branching, reward application, repeated
  level transitions, and persistence. Do not invent a combat requirement.

## Sources

- Retail data: `Data/res/strings/CHARGEN.STR` (`Mission1..27`, year labels, and
  exact rewards).
- Runtime tables/state machine: `shock2vr/src/career.rs`,
  `shock2vr/src/player_stats.rs`, `shock2vr/src/scripts/choose_service.rs`, and
  `shock2vr/src/scripts/choose_mission.rs`.
- Mission wiring: `cargo dq entities earth.mis ...` and
  `cargo dq entities station.mis ...` (2026-07-23), especially Station objects
  124–127, 132, 741, 745, and START roots 917–926/950.
- SDK characterization:
  `tools/shock2-sdk/test/station-flow.e2e.test.ts`,
  `station-career.e2e.test.ts`, and `station-career-attributes.e2e.test.ts`.
- [Official System Shock 2 manual](https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/238210/manuals/System%20Shock%202%20-%20Manual.pdf?t=1742502733)
  — recruitment facility, service branches, and career missions.
- [SShock2 walkthrough](https://www.sshock2.com/ss2walk/) — Earth service
  selection, three-door training years, and MedSci handoff.
- [GameBanshee character creation](https://www.gamebanshee.com/systemshock2/character/charactercreation.php)
  and [GameFAQs walkthrough](https://gamefaqs.gamespot.com/ps4/513865-system-shock-2-25th-anniversary-remaster/faqs/8959)
  — branch assignments and reward cross-check.
