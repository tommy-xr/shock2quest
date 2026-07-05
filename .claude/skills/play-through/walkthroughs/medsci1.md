# Walkthrough — medsci1 (MedSci Deck 1)

First playable deck. This walkthrough exercises the critical-path systems —
player state, item pickup, objective tracking, and the level transition — with
warp+teleport navigation.

**Derived from game data** (`cargo dq entities medsci1.mis ...`):
- The MedSci → Engineering transition is a `TrapTripLevel` tripwire, entity
  **764** (`sym:Tripwire`), `PropDestLevel("eng1")`, `PropDestLoc(21)`, trigger
  volume at **(12.684996, -5.635426, -43.591045)**. It fires on
  `SensorBeginIntersect` — teleporting the player into the volume triggers it.
- Pickup items exist as world entities (e.g. `20 Nanites`), resolvable at run
  time by name/template.
- Quest-bit entities here are mechanism triggers (elevator filters), not
  narrative objectives; medsci1's objectives are goal-driven, so CP3 demonstrates
  the quest-bit read/set path rather than asserting a specific mission objective.

**External cross-check:** the intended path on MedSci-1 is wake → arm yourself →
work through the deck → take the transition toward Engineering. The checkpoints
below abstract that into verifiable states.

Resolve item/trigger runtime ids at run time (`entities.list({filter})` /
`entities.byTemplate`) rather than trusting the ids above — they are stable but
the query is self-documenting and robust to data changes.

---

### CP1 — Spawn & load
- setup: launch the debug runtime on `medsci1.mis`; `step({frames: 5})`.
- act: none (pure state check).
- verify:
  - `info().mission == "medsci1.mis"`
  - `info().player.hit_points` is non-null and `> 0` (player has a health pool)
  - `player.position()` is all-finite
  - `entities.list({limit:1}).total_count > 0` (world instantiated)
- capture: screenshot `cp1-spawn.png`
- on-fail: mission won't load / no player / no entities → `[functionality]`
  (level load or entity instantiation). A null health pool → `[gameplay]` or
  `[debug_runtime]` depending on whether the player truly has no `PropHitPoints`.

### CP2 — Acquire an item (pickup)
- setup: `const item = entities.list({filter:"*Nanites*", limit:50}).entities[0]`
  (fall back to `*Wrench*` if none). Assert one was found.
- act: `player.give(item.id)`.
- verify:
  - the give call succeeds
  - `player.inventory()` contains an item with `entity_id == item.id` and
    `location == "inventory"`
- capture: screenshot `cp2-inventory.png` (open inventory if the HUD supports it)
- on-fail: give rejects a valid pickup → `[gameplay]` (frob eligibility) or
  `[debug_runtime]` (give lever). Item doesn't appear in inventory →
  `[debug_runtime]` (inventory read) or `[gameplay]` (Contains link).

### CP3 — Objective tracking (quest-bit path)
- setup: none.
- act: `quests.set("medsci.playthrough.cp3", "complete")`.
- verify:
  - `quests.get("medsci.playthrough.cp3") == "complete"`
  - `quests.list()` includes it; resetting it to `"unknown"` removes it
- capture: none.
- on-fail: quest read/set disagree → `[debug_runtime]` (quest endpoint). (When a
  real medsci1 objective bit is identified, replace this with: perform the
  objective action, then assert the game-set bit flips — a `[gameplay]` check.)

### CP4 — Level transition (MedSci → Engineering)
- setup: `player.teleport({x:12.684996, y:-5.635426, z:-43.591045})` — into the
  entity-764 tripwire volume.
- act: `step({frames: 15})` to let `SensorBeginIntersect` fire.
- verify:
  - `info().mission == "eng1.mis"` (the transition fired and eng1 loaded)
  - after `step({frames: 30})`, `player.position()` is all-finite (player is
    live in the new level)
- capture: screenshot `cp4-eng1.png`
- on-fail: mission stays `medsci1.mis` → `[gameplay]` (tripwire/`TrapTripLevel`
  didn't fire) — cross-check by warping directly (`transitionLevel("eng1", 21)`);
  if the direct warp works but the trigger doesn't, it's the trigger. Runtime
  crashes on transition → `[functionality]` (level load, e.g. the known
  shodan.mis loader crash class).

---

## Notes for future (driving the player)

CP targets are world positions, so a later upgrade can replace "teleport to CP"
with "drive to CP" (thumbstick + step) to additionally exercise navmesh,
collision, and locomotion over the same route — do this only once the
warp+teleport run is green end-to-end.
