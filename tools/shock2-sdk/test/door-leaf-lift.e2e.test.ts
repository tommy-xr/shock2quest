import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary } from "../src/types.js";

// A creature beside a travelling door leaf must stay on the floor (#1255).
//
// medsci1's Security Door (template 254) is a leaf that rises out of the
// doorway. A grunt pressed against its face makes a contact whose normal is
// horizontal, and Rapier's friction constraint then works to erase the
// relative tangential velocity between the two surfaces - which for a leaf on
// the move is entirely vertical. The capsule is dragged along with the leaf.
//
// The lift is only evidence if the grunt actually met the leaf, so the run
// asserts a narrow-phase contact between the two bodies while the leaf travels
// (GET /v1/physics/bodies/:id reports which bodies a body touches). Proximity
// would not do: the AI holds a stand-off from a door and can satisfy a
// distance check without ever touching the leaf. The travel window the grunt
// is reliably inside of is the leaf shutting on it while it stands in the
// doorway; the friction is the same one either way, and the physics unit test
// `a_rising_kinematic_leaf_does_not_lift_a_creature_against_its_face` covers
// the rising direction with contact guaranteed by construction.
//
// Negative-first: with the contact hook disabled this run reports the grunt in
// contact with the travelling leaf and carried well off its resting y.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The rising Security Door: corridor along Z, leaf closed plane z = -4.008. */
const DOOR = { x: -18.4047, y: 0.5, z: -4.0081162 };
/** SpawnDebugMonster drops the grunt 4 units along the look direction. */
const SPAWN_STAND = -7.62;
/** Contact tolerance: settling on the floor moves the body by ~0.005. */
const LIFT_TOLERANCE = 0.05;

function body(bodies: PhysicsBodySummary[], label: string): PhysicsBodySummary {
  assert.equal(bodies.length, 1, `expected one ${label} body, got ${bodies.length}`);
  return bodies[0];
}

test(
  "medsci1.mis: a travelling door leaf does not carry the creature against it",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    const doors = (await game.entities.list({ filter: "Security Door" })).entities.filter(
      (entity) =>
        Math.abs(entity.position[0] - DOOR.x) < 0.2 &&
        Math.abs(entity.position[2] - DOOR.z) < 0.2,
    );
    assert.equal(doors.length, 1, "expected exactly one Security Door on the medsci1 door line");
    const door = doors[0];

    // Spawn a grunt on the near (-Z) side of the door and let it stand up.
    await game.player.teleport({ x: DOOR.x, y: -1.0, z: SPAWN_STAND });
    await game.input.set("head.look", [-90.0, 0.0]);
    await game.step({ frames: 4 });
    const before = new Set(
      (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities.map(
        (entity) => entity.id,
      ),
    );
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 2 });
    const spawned = (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities.filter(
      (entity) => !before.has(entity.id),
    );
    assert.equal(spawned.length, 1, "SpawnDebugMonster should add exactly one grunt");
    const grunt = spawned[0];
    await game.step({ frames: 300 });

    // The player crosses to the far side and shuts the door behind them, so
    // the grunt has to come through the doorway to reach them.
    await game.player.teleport({ x: DOOR.x, y: -1.0, z: -1.2 });
    await game.entities.sendMessage(door.id, { type: "TurnOff" });
    await game.step({ frames: 180 });

    const resting = body(
      (await game.physics.bodies({ entityId: grunt.id })).bodies,
      "grunt",
    ).position[1];
    const shut = body((await game.physics.bodies({ entityId: door.id })).bodies, "door");
    assert.ok(
      Math.abs(shut.position[1] - DOOR.y) < 0.05,
      `the leaf must start shut, got y=${shut.position[1]}`,
    );

    await game.input.trigger("DebugForceChase");
    let peakWhileTravelling = resting;
    let leafTravelFrames = 0;
    let touchedTheTravellingLeaf = false;
    let crossed = false;
    // AI path queries run on a worker thread, so the frame the grunt reaches
    // the door jitters. Run until it is through and has met the leaf, rather
    // than for a fixed count. The leaf travels twice over this window: up as
    // the grunt approaches, and down again when the door closes itself behind
    // it - the grunt is standing in the doorway for the second one.
    for (let frame = 0; frame < 900 && !(crossed && touchedTheTravellingLeaf); frame += 1) {
      await game.step({ frames: 1 });
      const leaf = body((await game.physics.bodies({ entityId: door.id })).bodies, "door");
      const actor = body((await game.physics.bodies({ entityId: grunt.id })).bodies, "grunt");
      if (Math.abs(leaf.velocity[1]) > 0.1) {
        leafTravelFrames += 1;
        peakWhileTravelling = Math.max(peakWhileTravelling, actor.position[1]);
        // Without this the lift assertion is vacuous: a grunt that never meets
        // the leaf while it travels cannot be carried by it either. Proximity
        // is not enough - the AI holds a stand-off from a door - so ask the
        // narrow phase whether the two bodies are actually touching.
        if (!touchedTheTravellingLeaf && Math.abs(actor.position[2] - DOOR.z) < 2.5) {
          const detail = await game.physics.body(actor.body_id);
          touchedTheTravellingLeaf = (detail.contacts ?? []).includes(leaf.body_id);
        }
      }
      // The grunt starts at z < -5 and the player is at z = -1.2.
      if (!crossed && actor.position[2] > DOOR.z + 0.5) {
        crossed = true;
        // It is standing in the doorway now. Pull the player back to the far
        // side and shut the door on it: that is the second travel window, the
        // one the grunt is guaranteed to be inside of - and it keeps the grunt
        // off the player, which would otherwise end the mission mid-run.
        await game.player.teleport({ x: DOOR.x, y: -1.0, z: SPAWN_STAND });
        await game.entities.sendMessage(door.id, { type: "TurnOff" });
      }
    }

    assert.ok(leafTravelFrames > 30, `the leaf should travel; saw ${leafTravelFrames} frames`);
    assert.ok(
      touchedTheTravellingLeaf,
      "the grunt must be in contact with the leaf while it travels, or the lift assertion proves nothing",
    );
    assert.ok(
      peakWhileTravelling - resting < LIFT_TOLERANCE,
      `the travelling leaf must not carry the grunt: rest y=${resting}, peak y=${peakWhileTravelling}`,
    );
    assert.ok(crossed, "the grunt should walk through the doorway rather than ride the leaf");

    // Let it come to a stop: a body mid-stride loses and regains its floor
    // contact frame to frame, so "is it supported" is only a question worth
    // asking once it has settled.
    await game.step({ frames: 120 });
    const settled = body((await game.physics.bodies({ entityId: grunt.id })).bodies, "grunt");
    assert.ok(
      Math.abs(settled.position[1] - resting) < LIFT_TOLERANCE,
      `the grunt must end back on the floor, rest y=${resting}, ended y=${settled.position[1]}`,
    );
    const settledDetail = await game.physics.body(settled.body_id);
    assert.ok(
      settledDetail.contact_count > 0,
      "the grunt must end touching the world, not hanging in the air",
    );
    // Contacts alone say "touching something", not "standing on something".
    // The floor directly under the capsule is the support: probe for it, and
    // require it right below the body rather than somewhere further down.
    const support = await game.raycast({
      start: [settled.position[0], settled.position[1] + 0.5, settled.position[2]],
      end: [settled.position[0], settled.position[1] - 3.0, settled.position[2]],
      collision_groups: ["world", "entity"],
    });
    assert.ok(support.hit, "no floor under the settled grunt - it is unsupported");
    assert.ok(
      support.hit_point !== null && settled.position[1] - support.hit_point[1] < 0.5,
      `the grunt must be standing on the floor, not hovering above it: body y=${settled.position[1]}, floor y=${support.hit_point?.[1]}`,
    );
  },
);
