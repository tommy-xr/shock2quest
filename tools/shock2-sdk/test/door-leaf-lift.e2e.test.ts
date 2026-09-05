import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary } from "../src/types.js";

// A creature under a rising door leaf must stay on the floor (#1255).
//
// medsci1's Security Door (template 254) is a leaf that rises out of the
// doorway. A grunt pressed against its face makes a contact whose normal is
// horizontal, and Rapier's friction constraint then works to erase the
// relative tangential velocity between the two surfaces - which for a rising
// leaf is entirely vertical. The capsule is dragged up with the leaf.
//
// Negative-first: on the parent commit this run lifts the grunt from its
// resting y = 0.099 to y = 1.719 while the leaf travels, leaves it hanging
// there for ~0.75 s, and only then drops it. The assertions below are on the
// lift, so they fail on the parent and pass with the contact hook.
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
  "medsci1.mis: a rising door leaf does not carry the creature under it",
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
      (await game.entities.list({ limit: 4000 })).entities.map((entity) => entity.id),
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
    let crossed = false;
    for (let frame = 0; frame < 400; frame += 1) {
      await game.step({ frames: 1 });
      const leaf = body((await game.physics.bodies({ entityId: door.id })).bodies, "door");
      const actor = body((await game.physics.bodies({ entityId: grunt.id })).bodies, "grunt");
      if (Math.abs(leaf.velocity[1]) > 0.1) {
        leafTravelFrames += 1;
        peakWhileTravelling = Math.max(peakWhileTravelling, actor.position[1]);
      }
      // The grunt starts at z < -5 and the player is at z = -1.2.
      crossed ||= actor.position[2] > DOOR.z + 0.5;
    }

    assert.ok(leafTravelFrames > 30, `the leaf should travel; saw ${leafTravelFrames} frames`);
    assert.ok(
      peakWhileTravelling - resting < LIFT_TOLERANCE,
      `the rising leaf must not lift the grunt: rest y=${resting}, peak y=${peakWhileTravelling}`,
    );
    assert.ok(crossed, "the grunt should walk through the doorway rather than ride the leaf");

    const settled = body((await game.physics.bodies({ entityId: grunt.id })).bodies, "grunt");
    assert.ok(
      Math.abs(settled.position[1] - resting) < LIFT_TOLERANCE,
      `the grunt must end back on the floor, rest y=${resting}, ended y=${settled.position[1]}`,
    );
    const settledDetail = await game.physics.body(settled.body_id);
    assert.ok(
      settledDetail.contact_count > 0,
      "the grunt must end supported, not hanging in the air",
    );
  },
);
