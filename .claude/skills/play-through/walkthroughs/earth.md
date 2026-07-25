# Walkthrough — Earth (tram → training → service choice → station)

Context for the `playtest` agent and the `play-through` reviewer. This is the
complete Earth character-creation route, including the optional training that a
normal player can skip. It is not a coordinate script: use signs, doors,
instructors, visible objects, and screenshots to navigate. The positions below
are recovery anchors and evidence checks, not permission to teleport past a
world interaction.

Earth has two different completion standards:

- **Campaign progression:** ride the correct gravshaft, choose one service, and
  arrive in `station.mis` with that career applied.
- **Earth depth pass:** do all of the above **and** genuinely exercise Basic
  Training and all three Advanced Training rooms before recruitment. This is
  the standard for the current whole-game play-through.

The basketball secret and completing station transitions for all three careers
are useful focused checks, but they are not required in one advancing campaign
run. Entering any career tripwire leaves Earth, so one run cannot honestly
transition through all three.

Mission object ids and positions below are stable in `earth.mis`; runtime
entity ids are not. Rediscover runtime entities by name and `template_id` after
every launch or transition.

## Route overview

1. Leave the UNN tram and cross the boardwalk.
2. Use the **left / x≈9.6 UP gravshaft**, not the right-hand DOWN shaft.
3. Complete Basic Training.
4. Enter the west-side Advanced Training lobby and complete Weapons, Technical,
   and Psionic training.
5. Return to the hub, open the north-side service passage's real doors, and
   reach the career concourse.
6. Inspect the Marine, Navy, and OSA entrances, choose one, and walk through
   its tripwire.
7. Verify `station.mis`, the mutually exclusive career quest bit, the designed
   station arrival, and the branch's starting HP/psi.

## Phase 1 — tram and gravshaft

The fresh Earth spawn is inside the tram at approximately
**(10.7, 1.5, −11.3)**.

1. Let the player settle, then walk forward out of the tram. Continue across
   the boardwalk to the gravshaft wall around z≈12–14. A real traversal must
   move from z<−10 to z>10; teleporting from the tram does not cover the known
   floor-contact regression.
2. Enter the shaft on the **left when facing the pair from the tram**, centered
   near **x=9.6**. This is room `Up` (mission object 270) and negative room
   gravity should carry the player from y≈2 to street level above y=18.
3. Exit when the top `Stop` room cancels the lift. The neighboring shaft at
   **x≈12.0** is room `Down` (object 227); standing there at the bottom should
   not lift the player.

Use “x≈9.6 UP shaft” in evidence because retail walkthroughs describe the shaft
from differing viewpoints and therefore disagree on the word “left/right.”

Optional secret: the basketball on the arch near the gravshafts is a legitimate
exploration pickup. Record it if reachable through normal jumping/climbing, but
do not let it delay the critical route or use a debug give to claim the secret.

## Phase 2 — Basic Training (optional to retail, required for depth)

Follow the visible **Basic Training** signs and enter the training area before
choosing a service. The sign entity is mission object 392
(`Basic_Training_Sign`) at approximately **(11.6, 27.3, 58.0)**.

The Basic entrance is the centered north passage at x≈11.6. Tripwire 359 opens
door leaves 358/368 around z≈59.6; the actual entry tripwire is object 378 near
**(11.56, 24.25, 66.65)**. It teleports the player to the authored Basic course
near **(5.6, 22.2, 237.4)**. This is Basic Training—not a recruitment interview
or a years-of-service montage.

The purpose is to prove the tutorial mechanics, not merely visit the rooms.
Follow the instructor prompts and demonstrate each available lesson:

1. Pick up the supplied object through a normal world interaction.
2. Open the inventory, select and move/use an inventory item, and verify the
   item actually appears in the inventory rather than relying on the pickup
   animation alone.
3. Frob a wall control or switch and observe the linked world response.
4. Move/select the supplied item in the inventory. Do not invent an
   item-on-item combination: the mission authors no second Basic combine
   target, and Fermium has no effectful use in this course.
5. Examine an object and open/read or play the supplied log/data item.
6. Traverse the movement lesson, including its ladder/platform and
   jump/mantle/climb affordance, without teleporting across it.
7. Leave by the real exit tripwire (object 380). Its linked player teleport
   trap 379 must return the player once to approximately
   **(11.61, 24.26, 65.79)**, without an automatic re-entry loop.

The course begins behind timed fields. Wait for the authored delays, operate
the highlighted Simple Button through player Frob, cross the lesson tripwires,
climb the real ladder, and traverse the upper platform. Do not damage or
teleport through the fields.

Two authored objective anchors must be deliberately sought rather than inferred
from the main path:

- Audio Log object **301** at approximately
  **(11.51, 21.63, 263.72)** (`PropLog { deck: 1, email: 33, log: 24 }`).
  Pick it up normally and verify that its log/playback state is exposed.
- Crate object **307** at approximately **(19.24, 21.60, 264.63)**, whose
  `Contains` link holds Fermium object **242**. Open the crate, transfer the
  contents normally, and inspect/use the inventory item as the lesson requests.

Training narration/text can be absent while the physical lesson still works.
In that case, deliberately search for the lesson, attempt its normal
interaction if reachable, and report the missing/inaccessible objective
separately. Do not call Basic “fully covered” after only completing its
button/ladder course.

## Phase 3 — all three Advanced Training rooms

Follow the **Advanced Training** sign (mission object 393,
`Advanced_Training_Sign`, around **(6.4, 27.3, 51.2)**). Visit all three labeled
rooms. The order is flexible, but evidence must distinguish them.

From the hub, enter the west/lower-x passage centered around z≈51.2. Door
leaves 74 and 79 are controlled by the large approach tripwire 383. Approach
on-center, allow roughly 20 simulation frames for both leaves and their
colliders to finish sliding to z≈48.8/53.6, then cross toward lower x. A move
attempt made off-center or while the leaves are still opening is tester error,
not a blocker.

The three lobby entry tripwires are arranged north/south around x≈−15.24:

| Room | Entry tripwire | Authored destination | Exit tripwire / return |
| --- | --- | --- | --- |
| Weapons | 313, z≈50.34 | trap 319, (77.90, 23.2, 183.82) | 374 → trap 371, z≈50.46 |
| Technical | 314, z≈56.74 | trap 324, (160.64, 22.8, 182.14) | 375 → trap 372, z≈56.81 |
| Psionic | 320, z≈63.16 | trap 325, (215.22, 24.0, 191.87) | 376 → trap 373, z≈63.16 |

Walk through each real entry and real exit. Do not use those coordinates as
raw teleports. The exit traps intentionally vaporize the temporary training
inventory; prove each exercise before leaving its room.

Each lobby aperture has an authored **0.8-world-unit raised threshold**. Back
off, center on the room's tripwire z coordinate, face west, and use sustained
normal locomotion so the character controller can autostep onto it. The debug
`/v1/player/move` helper performs a direct capsule shape cast without autostep,
so a blocked helper move is not evidence that these entrances are impassable.

### Weapons

The key authored objects are pistol 246, clips 247–251, Training Droid 547,
laser pistol 253, and recharging station 258.

1. Pick up the supplied pistol and ammunition. The pistol starts empty.
2. Equip the pistol, load it through normal inventory/weapon handling, and
   fire at the training robot/target until there is visible hit or damage
   evidence.
3. Pick up and equip the supplied energy weapon.
4. Use the recharge station on that weapon and verify the charge increases.
5. Fire it at the training robot/target and observe an impact/damage response.

Merely giving the player a weapon, sending a debug `Damage` message, or firing
into empty space does not cover this room. Do not use generic Reload to
substitute for the laser station: the objective is specifically station-driven
energy recharge. Also record whether pistol reload actually consumes one of
the supplied clips and whether the real exit removes the temporary gear.

### Technical

The key authored objects are nanites 257, keypad 266 controlling door 265,
inside button 595, replicator 262, and output marker 284.

1. Pick up the supplied nanites and record the initial amount.
2. Frob the locked training keypad and enter the real hacking UI.
3. Complete the hack by making the required connected path; verify the target
   changes state and loot its contents normally.
4. Frob the replicator and buy an item with nanites. Verify both the delivered
   item and the nanite decrease.
5. Attempt the room's replicator hack if exposed, and record whether it changes
   the available inventory. Treat an absent or nonfunctional hacking mechanic
   as a feature-gap finding rather than silently skipping it.

Opening the target through a message injection or supplying nanites through the
debug API invalidates this coverage. A numeric keypad without an authored code
does not satisfy the hacking objective.

### Psionic

The key authored objects are psi hypos 275/288/289, psi amp 290, and Training
Droid 593.

1. Pick up the supplied psi amp and psi hypo.
2. Equip/use them through normal player interaction.
3. Select the available Cryokinesis power through the normal UI/input path and
   manifest it at the Training Droid.
4. Require psi to decrease and the droid's health to decrease.
5. Use a psi hypo from inventory and require psi to recover.

Also record whether the authored room-entry psi adjustment occurred and whether
the exit removes the temporary amp/hypos. Simply holding the amp or cycling a
debug power selector is not completion.

If the current port cannot expose the intended psi selection/cast path, record
the first failed real interaction precisely and classify the room as uncovered.

## Phase 4 — exact route to the service concourse

This is the easy place to mistake collision for a broken route. The service
concourse is **not** the west-side Advanced lobby.

1. Return from the last Advanced room to its real lobby destination, then walk
   east/higher-x through doors 74/79 to the broad hub.
2. Locate the north/increasing-z service passage around x≈5. The
   `Interrogation Room` doors, objects
   **80** at about **(5.01, 24, 59.79)** and **81** at about
   **(5.00, 24, 63.38)**, have **no incoming SwitchLinks**. Center the reticle
   on each door and frob it. Wait for the panel and its collider to slide clear
   before walking through.
3. Once north of the second frobbed door, the passage opens into the three-way
   service concourse around z≈65–70.

Evidence must distinguish the tripwire-opened Advanced doors 74/79 from the
player-frobbed service doors 80/81. Teleporting to a career tripwire or
injecting `Frob` directly into a discovered door entity does not prove
navigation/player interaction.

## Phase 5 — inspect all services, then choose one

Before committing, visit the signed threshold of all three branches while
remaining outside their terminal tripwire. Record a screenshot/observation of
each sign and doorway:

- **Marines:** from the common concourse, take the western branch, then turn
  south. Terminal tripwire 309 is at approximately
  **(−2.50, 24.0, 68.16)**.
- **Navy:** continue north on the western side. Terminal tripwire 217 is at
  approximately **(0.51, 24.0, 84.06)**.
- **OSA:** take the eastern/northeastern branch, continue north, then east.
  Terminal tripwire 218 is at approximately
  **(16.86, 24.0, 80.75)**.

Do not step across a threshold while scouting: each ENTER signal immediately
fires `TrapNewTripwire → SwitchLink → ChooseService` and transitions to
`station.mis`.

Choose the intended career only after all three entrances have been identified.
Walk through its threshold and allow the transition to finish:

| Branch | Mission wiring | Required station evidence |
| --- | --- | --- |
| Marine | tripwire 309 → marker 311, `PropService(0)` | `career_marine=complete`; Navy/OSA unknown; max HP 45, max psi 20 |
| Navy | tripwire 217 → marker 609, `PropService(1)` | `career_navy=complete`; Marine/OSA unknown; max HP 35, max psi 35 |
| OSA | tripwire 218 → marker 219, `PropService(2)` | `career_osa=complete`; Marine/Navy unknown; max HP 30, max psi 60 |

The Navy destination marker is misleadingly named `SendToMarines` in retail
data. Judge the branch by `PropService(1)` and `career_navy`, not that editor
name.

All three markers target `station.mis`, destination location 2501. A successful
transition arrives near the authored station start at approximately
**(81.78, −3.6, 16.54)**. Immediately after arrival, `player.stats` should
exist and `granted_years` should still be empty; station tours have not yet
been completed.

## Strict acceptance checklist

An **Earth depth pass** is valid only when one continuous fresh campaign session
shows all of:

- The player walks from the tram to the gravshafts and rides the x≈9.6 UP shaft
  to street level.
- Basic Training is entered and its supplied pickup, inventory/item, switch,
  inspect/log, and movement interactions are attempted genuinely; any
  unavailable lesson is reported at its first failed interaction.
- Weapons Training proves every numbered lesson above: the supplied pistol and
  clips are acquired normally; reload consumes matching reserve ammunition;
  pistol fire damages the real training target; the supplied laser is charged
  by the authored station rather than a generic reload/debug effect; and laser
  fire damages the target.
- Technical Training proves every numbered lesson above: the supplied nanites
  are acquired; the locked target is opened through the real connected-path
  hacking UI; a replicator purchase delivers the selected item and debits the
  correct nanites; and the replicator hack changes its available inventory.
- Psionic Training proves every numbered lesson above: the supplied amp and
  hypo are acquired and used normally; Cryokinesis is selected and manifested
  at the real Training Droid; psi and droid health both decrease; and a psi
  hypo restores psi.
- Each Advanced room's real exit returns to the lobby. Before/after inventory
  evidence must prove that its temporary weapon/ammo, technical supplies, or
  amp/hypos were removed by the authored exit, rather than merely recording
  whether cleanup happened.
- The Basic course is allowed to run through its timers, its course is
  traversed under collision, its exit tripwire fires, and the player returns
  exactly once. Before/after inventory evidence proves that the Basic course's
  temporary supplied items were removed by its authored exit.
- From the hub, the player enters the west-side Advanced lobby through
  tripwire-opened doors 74/79, later returns east to the hub, and player-frobs
  service-passage doors 80/81.
- Marine, Navy, and OSA entrances are all located and observed before one is
  selected.
- One real career threshold is crossed; `station.mis` loads at the designed
  area, exactly one matching `career_*` bit is complete, and its HP/psi values
  match the table. The initial Station inventory baseline must contain no
  leaked Earth training gear or supplies.
- No `/v1/player/teleport`, direct transition, quest mutation, entity-message
  injection, debug give, or collision bypass substitutes for any gate above.
  A teleport may restore a previously validated saved frontier, but that
  resumed evidence cannot be used to claim the skipped Earth traversal.

A **minimum campaign pass** may omit all training and visit only the chosen
service entrance, because retail permits that. Label such a result
`progression-only`; do not present it as the requested Earth depth pass.

If any required Advanced interaction or exit cleanup fails, the Earth depth
verdict is **BLOCKED at that interaction** even when the player could walk on
to recruitment. Record and triage the failure; do not downgrade it to a passing
run with a side finding.

Focused branch replays may start from a saved pre-choice frontier to validate
the two unchosen service transitions. Each replay must still walk through the
real threshold and verify its own quest/stats; directly firing the destination
marker is only a wiring diagnostic.

## Engine watch-points for triage

1. The tram floor and the UP gravshaft are known regression surfaces. Report
   the exact position and movement trace if either ordinary walk or room
   gravity stalls.
2. Both the Basic entrance at x≈11.6 and the Advanced entrance around
   x≈5/z≈51.2 can pin an off-center player while their leaves move. Wait for
   the doors to reach their authored open endpoints and retry on-center before
   reporting a blocker.
3. The scripted Basic-course return must suppress stale entry-tripwire tracking.
   More than one automatic return/re-entry is a teleport-loop regression.
4. Low Basic-course walls require observed steering. A long held input into a wall
   is a test-navigation failure unless bounded corrective attempts prove the
   passage itself is impassable.
5. Doors 80/81 are intentionally player-frobbed `StdDoor`s with no incoming
   SwitchLinks. Waiting for an automatic trigger is tester error; a correctly
   targeted player frob that does not move both the panel and collider is a
   gameplay bug.
6. Several Earth tutorial prompts use `EarthText`; the current script registry
   may not reproduce all retail narration/UI. Separate missing presentation
   from the underlying pickup, inventory, weapon, hack, psi, and movement
   mechanics, and report both where appropriate.

## Sources

- [SShock2.com full walkthrough](https://www.sshock2.com/ss2walk/) — tram,
  training recommendation, recruitment, and career choice.
- [RPGClassics System Shock 2 walkthrough](https://shrines.rpgclassics.com/pc/sysshock2/walkthrough.shtml)
  — independent confirmation that Basic and all three Advanced Training rooms
  are optional but recommended.
- [GameBanshee character creation guide](https://www.gamebanshee.com/systemshock2/character/charactercreation.php)
  — career roles and their effect on starting character development.
- Local `cargo dq entities earth.mis` inspection (2026-07-23) — gravshaft
  rooms, signs, training and service-passage doors, tripwires, destination
  markers, service values, and station location.
- `tools/shock2-sdk/test/earth-tram-exit.e2e.test.ts`,
  `gravshaft.e2e.test.ts`, `door-frob.e2e.test.ts`,
  `montage-teleport-loop.e2e.test.ts`, and `station-flow.e2e.test.ts` — current
  runtime regression evidence and observable state.
