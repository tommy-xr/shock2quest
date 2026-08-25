import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, PhysicsBodySummary } from "../src/types.js";

// Regression coverage for #597. In the Dark Engine, P$PhysDims is an
// instantiated, non-inherited property: mission objects can inherit P$PhysType
// without storing dimensions of their own. The original engine materializes
// those instance dimensions from the model bounds when it loads the object.
//
// Runtime entity ids vary between launches, so fixtures are discovered by the
// stable mission-object id exposed as `template_id`.
//
// Negative-first: before #597, station pipe 583 and hydro2 pipe 706 had no
// rigid body because neither stores P$PhysDims in the mission. The positive
// assertions below therefore fail on main. Hydro2 hand rail 1007 is the
// control: its exact PhysType is None, so its render model must not make it
// physical.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function exactlyOne(
  game: GameServer,
  missionObjectId: number,
  description: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(missionObjectId);
  assert.equal(
    matches.length,
    1,
    `expected exactly one ${description} (mission object ${missionObjectId})`,
  );
  return matches[0];
}

async function modelBoundsBody(
  game: GameServer,
  missionObjectId: number,
  description: string,
): Promise<PhysicsBodySummary> {
  const entity = await exactlyOne(game, missionObjectId, description);
  const { bodies } = await game.physics.bodies({ entityId: entity.id });
  assert.equal(
    bodies.length,
    1,
    `${description} should receive one collider from its model bounds`,
  );
  const body = bodies[0];
  assert.equal(body.entity_id, entity.id);
  assert.equal(body.body_type, "kinematic");
  assert.equal(body.is_enabled, true);
  assert.equal(body.is_sensor, false);
  assert.ok(body.collision_groups.includes("entity"));
  return body;
}

test(
  "station.mis: dimensionless structural pipes receive model-bounds colliders",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
    });
    await game.step({ frames: 2 });

    await modelBoundsBody(game, 583, "station Pipe 16x2");
  },
);

test(
  "hydro2.mis: model-bounds fallback respects supported and None physics types",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 2 });

    await modelBoundsBody(game, 706, "hydro2 Pipe 16x2");

    const handRail = await exactlyOne(game, 1007, "hydro2 Hand Rail 8'");
    const { bodies } = await game.physics.bodies({ entityId: handRail.id });
    assert.equal(
      bodies.length,
      0,
      "an exact PhysType None must remain bodyless even when it has a model",
    );
  },
);
