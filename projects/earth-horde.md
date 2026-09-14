# Earth containment experiment

Run `cargo dbgr --mission earth_horde` (or launch that mission in the desktop runtime). `earth_horde_test` uses three short waves for iteration. The aliases load Earth geometry; ordinary `earth.mis` keeps its original behavior.

Ten escalating waves have about 26 minutes of minimum scheduled combat and rest. Clearing enemies can take longer. After the final wave, use the ready button beside the shops to begin optional endless play. The same button skips a rest. Doors are closed and locked and the training trigger graph is disabled.

The starter backpack contains a wrench, pistol, psi amp, ammunition and medical/psi supplies. Shops are on the street, trainers upstairs. Buy psi tiers and individual powers separately; only powers supported by the current runtime are sold. Replicated items dispense in front of the machines. Random equipment and currency supplement normal enemy loot and wave rewards.

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

When protection expires, overlapping Hydro growth spreads outward from shop and
trainer approaches onto nearby floors and walls, then farther along travel routes.
There are 339 surveyed patches: 110 in the subway, 155 on the street, and 74
upstairs. Sites were checked against world geometry and reachable arena paths;
locked training rooms are excluded. Growth is cosmetic and does not prevent
using the machines. Wall eggs remain a follow-up.

Density reaches its maximum after 90 unprotected combat seconds. At 60 seconds,
the zone can produce a GrubEgg every 45 combat seconds. Eight floor sites per
zone prioritize services before extending along routes, with at most four pods
per zone. Approaching within four world units hatches an existing pod even during
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
and egg production tenfold for diagnostics, while keeping recovery/hatching rules.

The first pass deliberately uses floor GrubEggs. Wall-mounted growth reuses the
Hydro meshes with wall-facing transforms; wall eggs and additional payloads can
follow once pacing has been playtested.
