import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

// medsci1's Maintenance droid (stable mission object id) and the parts its
// archetype's CorpsePart links name.
const MAINTENANCE_DROID = 87;
const PARTS = ["DroidRightArm", "DroidLeftArm", "DroidCanopy", "DroidUpper"];

test("a destroyed droid leaves its CorpsePart wreckage on the floor", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 10 });
  const [droid] = await game.entities.byTemplate(MAINTENANCE_DROID);
  assert.ok(droid, "medsci1 places a Maintenance droid");

  await game.entities.sendMessage(droid.id, { type: "Damage", amount: 1000 });
  await game.step({ frames: 150 });

  for (const name of PARTS) {
    const [part] = (await game.entities.list({ filter: name })).entities.filter(
      (e) => e.name === name,
    );
    assert.ok(part, `${name} spawned`);
    const distance = Math.hypot(
      part.position[0] - droid.position[0],
      part.position[2] - droid.position[2],
    );
    assert.ok(distance < 5, `${name} lands near the droid (${distance.toFixed(2)})`);
    const [body] = (await game.physics.bodies({ entityId: part.id })).bodies;
    assert.equal(body?.body_type, "dynamic", `${name} is simulated debris`);
    // Parts spawn ~1 unit above the droid's origin; settled ones rest lower.
    assert.ok(part.position[1] < droid.position[1], `${name} fell to the floor`);
  }
});
