# Earth containment experiment

Run `cargo dbgr --mission earth_horde` (or launch that mission in the desktop runtime). `earth_horde_test` uses three short waves for iteration. The aliases load Earth geometry; ordinary `earth.mis` keeps its original behavior.

The player starts at the top of the stairs, facing the subway gravshaft.

Ten escalating waves have about 26 minutes of minimum scheduled combat and rest. Clearing enemies can take longer. After the final wave, use the ready button beside the shops to begin optional endless play. The same button skips a rest. The training trigger graph is disabled. The original training doors remain sealed; OS rewards are available from the landing bank.

The starter backpack contains a wrench, pistol, psi amp, ammunition and medical/psi supplies. Three shops are spread across the street; four trainers are spread around the subway. Buy psi tiers and individual powers separately; only powers supported by the current runtime are sold. Replicated items dispense in front of the machines. Random equipment and currency supplement normal enemy loot and wave rewards.

Corpses and their remaining contents stay lootable throughout the rest. Starting the next wave removes them; items already collected survive. Run state, rewards and purchased powers persist in saves.

Containment adds renewable circulators, floor/wall growth, and proximity-hatching floor eggs. Optional ladders, wall eggs, and day/night lighting remain follow-ups. Balance is experimental; an independent authentic playtest reached a wave-one death after killing a hybrid, confirming combat and stair traversal but not a full-run completion.

Validation includes director unit tests for timing, final-wave/endless gating, rewards, corpse cleanup and serialization; psi purchase quote tests; and debug-runtime purchase/save checks. The short alias is a diagnostic aid, not evidence that a full-length run has been completed.

## Cumulative wave roster

Each wave begins with a pipe hybrid and a shotgun hybrid, then introduces the new types and guarantees one of every returning type. Remaining slots are weighted random reinforcements. At most 15 enemies are alive at once; both the timer and all kills must finish before rest.

| Wave | Total enemies | New types | Minimum assault |
|---|---:|---|---:|
| 1 | 6 | Pipe and shotgun hybrids | 1:00 |
| 2 | 9 | Maintenance and protocol droids | 1:08 |
| 3 | 12 | Midwife and blue monkey | 1:16 |
| 4 | 15 | Large spider (Arachnid) | 1:24 |
| 5 | 18 | Small spider (Baby Arachnid) | 1:32 |
| 6 | 22 | Security droid and red monkey | 1:40 |
| 7 | 26 | Assassin and grenade hybrid | 1:48 |
| 8 | 30 | Rumbler and assault droid | 1:56 |
| 9 | 34 | Overlord | 2:04 |
| 10 | 38 | Greater Overlord and SHODAN avatar | 2:12 |

Endless begins at wave 11 with 44 enemies and a 2:20 minimum assault. Each further wave adds six enemies and eight seconds; there is no gameplay cap on wave count or scheduled growth. The full roster remains guaranteed. Random reinforcements increasingly favor enemies introduced in waves 6–10 (the weighting reaches its maximum at wave 42). SHODAN appears once per wave, outside the random reinforcement pool; this is the standalone avatar, not the original shield/finale encounter. The initial diagnostic exposed a shared projectile-origin bug: the avatar animated but did not fire. [Independent projectile fix #1551](https://github.com/tommy-xr/shock2quest/pull/1551) restores its energy shots and damage, and corrects security/assault droid muzzle origins. That fix is intentionally separate from this mission change.

`cargo dbgr --mission earth_horde_final` starts at preparation for wave 10 for diagnosis and recording. It provides the usual starter character, not an earned late-game build; any debug stat/equipment provisioning for a recording must be disclosed. This alias does not demonstrate completion of waves 1–9.

Earth has 5,010 navigation cells and 20,581 links, split into multiple connected components. Spawn sites keep subway enemies separate from the street/lobby arena because enemies cannot use the player gravshafts. Stair traversal is runtime verified for hybrids; clearance and navigation of the expanded roster remain playtest targets.

## Wave music

Combat waves rotate the authored music from MedSci 1/2, Engineering 1/2,
Hydroponics 1/2, Operations 2/3/4, Recreation 1, Command 1/2 and Rickenbacker 1.
Wave 1 uses MedSci 1; selection is `(wave - 1) % 13`, so endless wave 14 returns
to MedSci 1. The start event repeats as needed to keep music going during a wave.
Preparation, rest, victory and failure stop the current clip immediately.
Loading a save restarts the saved assault's song from its beginning; other phases
load silently. Sample offsets and random music branches are not serialized.
## Renewable containment

Two Hydro air circulators protect the two levels. The subway cabinet protects
only the subway; the street cabinet protects both the street and upstairs lobby.
They begin with 180 and 210 seconds of protection respectively. Protection
counts combat time only: preparation, rest, and the completed-run screen do not
spend it. A warning appears with 30 seconds remaining.

Growth and new pods are gated until wave 4, even if the opening waves take a long
time. Protection still counts down during those waves. Once the wave gate is
open and protection expires, overlapping Hydro growth spreads outward from shop and
trainer approaches onto nearby floors and walls, then farther along travel routes.
There are 339 surveyed patches: 110 in the subway, 155 on the street, and 74
upstairs. Sites were checked against world geometry and reachable arena paths;
locked training rooms are excluded. Growth is cosmetic and does not prevent
using the machines. Wall eggs remain a follow-up.

Density reaches its maximum after 180 eligible unprotected combat seconds. At 120 seconds,
the zone can produce a GrubEgg every 45 combat seconds, up to four pods. At
150 seconds the cap rises to six and the interval drops to 30 seconds; at maximum
density it reaches eight pods with a 15-second interval. Each new spawn uses the
current interval; an already running countdown completes normally. Eight floor
sites per zone prioritize services before extending along routes. Approaching within four world units hatches an existing pod even during
rest, using the normal egg animation, sound, and grub behavior. Rest pauses
new egg production and deterioration, but does not make existing traps safe.
Eggs can be destroyed normally.

New runs begin with Toxin-A research completed. Buy ready-to-use Anti-Annelid
Toxin (Toxin-A) for 10 nanites at the supply replicator.
In flatscreen, frob a circulator while carrying a vial; in VR, release a held
vial against the cabinet. Each vial protects
that device’s level for another 180 combat seconds, immediately stops new eggs, and
clears accumulated patches over up to 12 seconds (recovery also runs during rest).
The cabinet changes from inactive `air_reof` to active `air_re`. Existing eggs
and grubs remain; servicing a circulator does not erase those threats.

Containment grubs have a separate global living limit of 24. They never consume
the director's 15 wave-enemy slots and are not required to complete a wave.
Hatching reserves available capacity within each batch. Open shells expire after
30 seconds; dead containment creatures join the existing end-of-rest corpse
cleanup. Saved runs retain each zone's protection, density, egg cadence, and
shell lifetime. The short `earth_horde_test` alias accelerates depletion, growth,
and egg production tenfold and bypasses the wave gate for diagnostics, while
keeping recovery/hatching rules.

The first pass deliberately uses floor GrubEggs. Wall-mounted growth reuses the
Hydro meshes with wall-facing transforms; wall eggs and additional payloads can
follow once pacing has been playtested.

## Pacing prototype controls

Open **Developer → Earth horde** in the pause menu or main menu. Both controls
apply live through the existing shared flat/VR developer menu and debug API:

| Key | Default | Meaning |
| --- | --- | --- |
| `horde_growth_wave` | 4 | First wave allowing growth and new pods (1–20). Protection still drains in earlier combat. |
| `horde_growth_seconds` | 180 | Unprotected combat seconds from bare to maximum growth (30–600). Larger is slower. |

Set these to `1` and `90` to compare the previous containment pacing. Increasing
the first-wave setting mid-run pauses new growth/pods below that wave; it does
not remove existing growth, pods, or grubs. Growth speed changes apply to remaining
growth immediately; Toxin-A recovery and egg cadence retain their existing speeds.
The diagnostic alias ignores only the wave gate, not the growth speed setting.

These are developer overrides: they reset when the application restarts and are
not stored in saves. Saved protection, density, and egg timers still resume; the
current process's tuning controls their subsequent progression.

Choose **Survive** on the main menu, select **Easy / Normal / Hard / Impossible**,
then **Start Game**. **Cancel** returns to the main menu. The choice initializes a
fresh horde run and stays fixed in its saves.

Horde pressure is separate from campaign difficulty. Campaign difficulty feeds
player health/psi pools and authored shop/training costs and loot; the director's
wave roster, quotas, living-enemy cap, and assault schedule are fixed independently.
A debug run accepts `--difficulty easy|normal|hard|impossible` at launch.

## OS bank and service placement

Four single-use OS stations mount on the continuous west wall of the upstairs
landing, grouped clear of the doorway and light fixture. The
first is available immediately; the others activate at the beginning of waves
3, 6, and 9. Offline stations are dimmed, show their wave requirement in the
object name, and refuse purchases until activated. Each grants one supported
OS trait for free. Used and unlocked state survives save/load. There are no
horde access cards or reward-room locks.

Trainers remain in the subway: stats at the west end, tech at the east end,
and weapons/psi on the east platform's back wall. The weapons trainer is clear
of the large containment cabinet. Three replicators serve the street: west,
south wall, and far east end. Their separate RepScreen displays and wall-mounted
height follow authored MedSci machines. All three cabinet backs contact their
walls; the south-wall ammunition shop avoids the sloped buttresses, vehicles,
and containment device. Outlets face accessible approaches.

## Start at a wave / jump cheat

Set **Developer → Earth horde → Start / jump to wave** (`horde_start_wave`,
1–100, default 1) before starting a fresh run. You begin with preparation for
that wave and the usual starter character. Alternatively, while playing, set
the same value and click **Developer → Cheats → Start selected horde wave**.
`DebugStartHordeWave` exposes the same action to the debug runtime input API.

A jump clears current wave attackers and their uncollected contents, resets
the wave schedule, starts the selected wave and its music, and enables any OS
stations due by that wave. It preserves inventory, bought upgrades and already
used stations. Skipped waves grant no nanites or cyber modules. Existing
infestation remains. Jumps require a living player in Earth horde; changing the
selector alone never rewinds a loaded or running game. The selector resets on
process restart, while the actual run's wave is saved normally.
## Intermission supplies

At initial preparation and each rest, two loose supplies and one cache appear
at random reachable sites on **each** floor. Caches randomly use an ordinary
TriOptimum crate or a security crate (one-third chance), with the existing hack
board, nanite/skill requirements, critical failures and ICE Pick support.
Every crate holds ammo, healing, psi and a nanite/cyber-module bonus.

Unclaimed drops last 90 simulation seconds, including time after you start the
next wave early. Pause freezes the clock. Collected/held items survive expiry;
remaining contents disappear with their expired crate. A new intermission
replaces older unclaimed drops. Saves retain the claim timer, contents, random
stream and one-time fill state. Fully emptied crates disappear immediately.

Wave attackers choose among sixteen surveyed sites, avoid their previous site
when alternatives exist, prefer cover, and keep eight units from the player.
Subway and street spawn arenas remain separate because enemies cannot ride the
gravshafts. Fresh runs randomize the stream; saves continue it.
## Talon turret builder

From wave 5, one non-attacking Talon installer appears on the street during
combat. It flies between three clear street sites, rotates while working, and
installs a hostile slug turret directly after 25 combat seconds. There are no
junction boxes. Destroy the Talon to interrupt unfinished work; it drops five
nanites and cannot be replaced until the next wave. Completed turrets remain
ordinary enemies and can be destroyed directly, freeing their sites.

Three sites cap living installations at three. Travel and construction pause
during rest; deployed turrets remain active. Builder position, work progress,
and replacement-wave gating survive save/load. World rays prevent flying
through walls. Developer → Earth horde offers builder enable, first wave and
build seconds. The diagnostic alias bypasses the wave gate and accelerates work
fivefold while travel remains normal. This prototype installs street turrets;
it does not deploy drones or choose arbitrary locations.

## Wave 10 survival report

Finishing wave 10 opens a **SURVIVED** battle report and pauses simulation until
an explicit choice. It shows enemies killed, actual damage taken, and enemy HP
lost. Enemy damage is an aggregate from all sources: the engine does not yet
attribute every damage event to a player. Healing, overkill and hits on dead
bodies do not inflate the totals. The same report canvas and buttons render in
flatscreen and VR; live gameplay messages are hidden while the menu is open.

**Continue — endless waves** keeps the character, equipment and arena, starts
wave 11, and resumes the existing ever-increasing schedule (six extra enemies
and eight extra assault seconds per subsequent wave). **Return to main menu**
ends the session. The report does not repeat after each endless wave. Battle
totals are updated with HP mutations and saved in the campaign state; loading
a victory save reopens the report. A fresh horde run resets its totals, while
wave-jump cheats preserve the totals actually earned and invent no statistics
for skipped waves. The short diagnostic alias reports survival after wave 3.
