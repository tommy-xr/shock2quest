import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

// Droids by stable mission object id, with the parts their archetype's
// CorpsePart links name. SecUpper authors its own PhysDims, unlike DroidUpper.
const CASES = [
  {
    mission: "medsci1.mis",
    droid: 87,
    parts: ["DroidRightArm", "DroidLeftArm", "DroidCanopy", "DroidUpper"],
  },
  {
    mission: "rec1.mis",
    droid: 740,
    parts: ["SecRightArm", "SecLeftArm", "SecCanopy", "SecUpper"],
  },
];

for (const { mission, droid: droidId, parts } of CASES) {
  test(`a destroyed ${mission} droid leaves its CorpsePart wreckage on the floor`, {
    skip: !enabled, timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission });
    await game.step({ frames: 10 });
    const [droid] = await game.entities.byTemplate(droidId);
    assert.ok(droid, `${mission} places droid ${droidId}`);

    await game.entities.sendMessage(droid.id, { type: "Damage", amount: 1000 });
    await game.step({ frames: 1 });
    // The droid's corpse is a delayed explosion that can fling the parts, so
    // placement is checked as they spawn and settling afterwards.
    const spawned = new Map<string, { id: number; position: number[] }>();
    for (const name of parts) {
      const [part] = (await game.entities.list({ filter: name })).entities.filter(
        (e) => e.name === name,
      );
      assert.ok(part, `${name} spawned`);
      const distance = Math.hypot(
        part.position[0] - droid.position[0],
        part.position[2] - droid.position[2],
      );
      assert.ok(distance < 2, `${name} spawns at the droid (${distance.toFixed(2)})`);
      spawned.set(name, part);
    }

    await game.step({ frames: 150 });
    for (const [name, { id, position }] of spawned) {
      const [body] = (await game.physics.bodies({ entityId: id })).bodies;
      assert.equal(body?.body_type, "dynamic", `${name} is simulated debris`);
      assert.ok(body.position[1] < position[1]! - 0.3, `${name} fell from where it spawned`);
    }
  });
}
