# Earth containment experiment

Run `cargo dbgr --mission earth_horde` (or launch that mission in the desktop runtime). `earth_horde_test` uses three short waves for iteration. The aliases load Earth geometry; ordinary `earth.mis` keeps its original behavior.

The player starts at the top of the stairs, facing the subway gravshaft.

Ten escalating waves have about 26 minutes of minimum scheduled combat and rest. Clearing enemies can take longer. After the final wave, use the ready button beside the shops to begin optional endless play. The same button skips a rest. The training trigger graph is disabled. Three existing office rooms use access-card locks; other training doors remain sealed.

The starter backpack contains a wrench, pistol, psi amp, ammunition and medical/psi supplies. Two shops are spread across the street; four trainers are spread around the subway. Buy psi tiers and individual powers separately; only powers supported by the current runtime are sold. Replicated items dispense in front of the machines. Random equipment and currency supplement normal enemy loot and wave rewards.

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

## Renewable containment

Three Hydro air circulators protect the subway, street, and upstairs independently.
The subway cabinet is beside the platform wall, the street cabinet is east of the
shops, and the upstairs cabinet is on the east wall opposite the trainers.
They begin with 180, 210, and 240 seconds of protection respectively. Protection
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
only that zone for another 180 combat seconds, immediately stops new eggs, and
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

Horde pressure is separate from campaign difficulty. Campaign difficulty feeds
player health/psi pools and authored shop/training costs and loot; the director's
wave roster, quotas, living-enemy cap, and assault schedule are fixed independently.
A debug run accepts `--difficulty easy|normal|hard|impossible` at launch.

## Exploration rewards

Clearing waves 2, 4, and 6 delivers the Subway office, Street office, and
Recruitment office access cards respectively. Cards appear in ACCESS and survive
save/load; each opens only its own existing door(s). The subway room uses mission
object 564, the street room 563, and the upstairs east room 365/367. The original
training trigger graph stays disabled inside them.

Each room contains one normal Trait Machine: choose one available OS upgrade
for free, once per station. The existing per-machine quest bit prevents another
purchase after leaving the room or loading a save. Essential ammo, healing, and
ordinary training remain outside these optional reward rooms.

Placement was surveyed against world walls. Trainers occupy west/east subway
ends and two separated back-wall sites; replicators use the street's west/east
wall panels, with their item outlets moved alongside them.

## Combat supplies prototype

Every wave scatters two small supplies among surveyed sites in the player's
current arena: ammo or psi, plus healing or nanites. Uncollected loose supplies
expire at the next wave; inventory, held items, and container contents survive.

Twenty combat seconds into each even-numbered wave, a TriOptimum supply cache
arrives at a random surveyed site in the current arena (three seconds in the
short diagnostic mode). Its message names the subway, street, or upstairs
landing. Search it normally for ammunition, healing, psi, and a random nanite/
cyber-module bonus. Only one unclaimed cache exists at a time; empty caches are
removed. A pending delivery waits for the previous cache to be looted, and rest
pauses delivery timers. Cache fill state, contents, timers, and random streams
persist across save/load.

Four additional spawn sites extend street and subway approaches. New sites were
checked for ground, headroom, and navigation to their respective arena centers.
Enemy spawning avoids the previous site whenever another eligible site exists,
still prefers cover, keeps eight units from the player, and respects the subway/
street separation. Fresh runs receive fresh random seeds; loaded runs continue
saved streams. The wave roster, quota, and live-enemy cap remain independently
controlled by the existing director.
