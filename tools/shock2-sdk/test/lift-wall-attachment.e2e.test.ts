import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// MedSci's visible lift-wall shell is a zero-volume physical attachment, not a
// dynamic prop or a solid model-bounds box. Runtime ids are rediscovered from
// the stable mission object ids every launch.
//
// Negative-first: before #479, the shell became a clamped 0.005-radius dynamic
// ball. PhysAttach rejected it because only kinematic bodies can be driven;
// when Lift 1 moved, contact launched the tiny body upward and rotated the
// visible shell away from the platform.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const LIFT_WALLS_OBJECT = 1852;
const LIFT_OBJECT = 1853;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

const delta = (after: Vec3, before: Vec3): Vec3 => [
  after[0] - before[0],
  after[1] - before[1],
  after[2] - before[2],
];

test(
  "medsci1.mis: zero-volume Lift 1 Walls anchor tracks the moving lift",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8216),
    });
    await game.step({ frames: 5 });

    const walls = only(
      await game.entities.byTemplate(LIFT_WALLS_OBJECT),
      "Lift 1 Walls mission object",
    );
    const lift = only(
      await game.entities.byTemplate(LIFT_OBJECT),
      "Lift 1 mission object",
    );
    const wallBodies = (await game.physics.bodies({ entityId: walls.id })).bodies;
    assert.equal(wallBodies.length, 1, "wall shell should retain one transform body");
    assert.equal(wallBodies[0].body_type, "kinematic");
    assert.deepEqual(
      wallBodies[0].collision_groups,
      [],
      "the authored zero-volume anchor must not invent collision geometry",
    );

    const liftBefore = (await game.entities.detail(lift.id)).position;
    const wallsBefore = (await game.entities.detail(walls.id)).position;
    await game.entities.sendMessage(lift.id, { type: "TurnOn" });
    await game.step({ frames: 120 });
    const liftAfter = (await game.entities.detail(lift.id)).position;
    const wallsAfter = (await game.entities.detail(walls.id)).position;
    const liftTravel = delta(liftAfter, liftBefore);
    const wallsTravel = delta(wallsAfter, wallsBefore);

    assert.ok(liftTravel[1] > 4, `Lift 1 should move upward, travel=${liftTravel}`);
    assert.ok(
      Math.hypot(
        wallsTravel[0] - liftTravel[0],
        wallsTravel[1] - liftTravel[1],
        wallsTravel[2] - liftTravel[2],
      ) < 0.05,
      `wall shell must match lift travel: walls=${wallsTravel}, lift=${liftTravel}`,
    );
  },
);
