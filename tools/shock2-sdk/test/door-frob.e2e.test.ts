import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// End-to-end coverage for the StdDoor script/effect/collider path after a Frob
// is dispatched. Earth mission objects 80/81 have no incoming SwitchLinks, so
// the StdDoor script itself must toggle. This uses normal player targeting and
// the production flat-mode squeeze interaction, not a script-message injection.
//
// Negative-first: before #495, MessagePayload::Frob was ignored by StdDoor, so
// both the entity transform and its kinematic body remained at z=63.38 and the
// recruitment-center passage stayed physically blocked.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const TARGET: Vec3 = [5.003685, 24.0, 63.38392];

function distanceSquared(entity: EntitySummary, target: Vec3): number {
  return entity.position.reduce(
    (sum, component, axis) => sum + (component - target[axis]) ** 2,
    0,
  );
}

test(
  "earth.mis: StdDoor Frob opens and closes its collider",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 5 });

    const candidates = (
      await game.entities.list({ filter: "Interrogation Room", limit: 20 })
    ).entities;
    assert.ok(candidates.length >= 2, "expected Earth's Interrogation Room door pair");
    const authoredDoor = candidates.reduce((nearest, candidate) =>
      distanceSquared(candidate, TARGET) < distanceSquared(nearest, TARGET)
        ? candidate
        : nearest,
    );
    assert.ok(
      distanceSquared(authoredDoor, TARGET) < 0.01,
      `expected recruitment door near ${JSON.stringify(TARGET)}, got ${JSON.stringify(authoredDoor.position)}`,
    );

    await game.player.teleport({ x: 5.0, y: 24.0, z: 57.0 });
    const visibleSurface = await game.raycast({
      start: [5, 25.6, 57],
      end: TARGET,
      collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
    });
    const door = candidates.find((candidate) => candidate.id === visibleSurface.entity_id);
    assert.ok(
      door,
      `expected the visible surface to belong to the door pair, got ${JSON.stringify(visibleSurface)}`,
    );
    const before = await game.entities.detail(door.id);
    const beforeBodies = (await game.physics.bodies({ entityId: door.id })).bodies;
    assert.equal(beforeBodies.length, 1, "the door should own one kinematic body");

    const aim = await game.player.aimAt(door);
    assert.equal(aim.classification, "surface", JSON.stringify(aim));
    assert.equal(aim.entity_id, door.id);
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 60 });

    const open = await game.entities.detail(door.id);
    const openBody = (await game.physics.bodies({ entityId: door.id })).bodies[0];
    assert.ok(
      Math.abs(open.position[2] - before.position[2]) > 1.2,
      `frob should slide the selected leaf open, before=${before.position[2]}, after=${open.position[2]}`,
    );
    assert.ok(
      Math.abs(openBody.position[2] - open.position[2]) < 0.01,
      "the kinematic collider must follow the opened door transform",
    );

    await game.player.aimAt(door);
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 60 });

    const closed = await game.entities.detail(door.id);
    const closedBody = (await game.physics.bodies({ entityId: door.id })).bodies[0];
    assert.ok(
      Math.abs(closed.position[2] - before.position[2]) < 0.01,
      `second frob should return the door to closed, got z=${closed.position[2]}`,
    );
    assert.ok(
      Math.abs(closedBody.position[2] - closed.position[2]) < 0.01,
      "the kinematic collider must follow the closed door transform",
    );
  },
);
