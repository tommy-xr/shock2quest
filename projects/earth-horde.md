# Earth containment experiment

Run `cargo dbgr --mission earth_horde` (or launch that mission in the desktop runtime). `earth_horde_test` uses three short waves for iteration. The aliases load Earth geometry; ordinary `earth.mis` keeps its original behavior.

Ten escalating waves have about 26 minutes of minimum scheduled combat and rest. Clearing enemies can take longer. After the final wave, use the ready button beside the shops to begin optional endless play. The same button skips a rest. Doors are closed and locked and the training trigger graph is disabled.

The starter backpack contains a wrench, pistol, psi amp, ammunition and medical/psi supplies. Shops are on the street, trainers upstairs. Buy psi tiers and individual powers separately; only powers supported by the current runtime are sold. Replicated items dispense in front of the machines. Random equipment and currency supplement normal enemy loot and wave rewards.

Corpses and their remaining contents stay lootable throughout the rest. Starting the next wave removes them; items already collected survive. Run state, rewards and purchased powers persist in saves.

This is the first playable slice. Circulators, Toxin-A replenishment/infestation, eggs, optional ladders and day/night lighting are not implemented yet. Toxin-A is stocked in preparation for the ecology increment. Balance is experimental; an independent authentic playtest reached a wave-one death after killing a hybrid, confirming combat and stair traversal but not a full-run completion.

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

Endless begins at wave 11 with 44 enemies and a 2:20 minimum assault. Each further wave adds six enemies and eight seconds; there is no gameplay cap on wave count or scheduled growth. The full roster remains guaranteed. Random reinforcements increasingly favor enemies introduced in waves 6–10 (the weighting reaches its maximum at wave 42). SHODAN appears once per wave, outside the random reinforcement pool; this is the standalone avatar, not the original shield/finale encounter. It is retained as an experimental test target: the avatar navigates, animates, takes damage and dies on Earth, but its ranged attacks currently produce no observed projectile or player damage. Do not count it as a verified combat boss.

`cargo dbgr --mission earth_horde_final` starts at preparation for wave 10 for diagnosis and recording. It provides the usual starter character, not an earned late-game build; any debug stat/equipment provisioning for a recording must be disclosed. This alias does not demonstrate completion of waves 1–9.

Earth has 5,010 navigation cells and 20,581 links, split into multiple connected components. Spawn sites keep subway enemies separate from the street/lobby arena because enemies cannot use the player gravshafts. Stair traversal is runtime verified for hybrids; clearance and navigation of the expanded roster remain playtest targets.
