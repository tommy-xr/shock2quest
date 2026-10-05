import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("Survive preserves generated pickup body placement across save/load", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde" });
  await game.step({ frames: 120 });
  const bodyPosition = async () => {
    const [tool] = await game.entities.byTemplate(-2949);
    assert.ok(tool, "the generated maintenance tool must exist");
    const [summary] = (await game.physics.bodies({ entityId: tool.id })).bodies;
    assert.ok(summary);
    const body = await game.physics.body(summary.body_id);
    return body.position;
  };
  const before = await bodyPosition();
  const saveName = `survive_supply_physics_${Date.now()}`;
  assert.ok((await game.save(saveName)).success);
  assert.ok((await game.load(saveName)).success);
  const after = await bodyPosition();
  assert.ok(Math.hypot(...after.map((v, i) => v - before[i])) < .02,
    `restoring a supply must preserve its body placement: ${before} -> ${after}`);
});

test("Survive grounds cyber modules and generated crates on their local floor", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde" });
  await game.step({ frames: 120 });
  const items = (await game.entities.list()).entities.filter(e =>
    e.location === "world" && [-938, -941, -1886].includes(e.template_id));
  assert.ok(items.some(e => e.template_id === -938), "starter cyber module exists");
  assert.equal(items.filter(e => [-941,-1886].includes(e.template_id)).length, 2);
  for (const item of items) {
    const detail = await game.entities.detail(item.id);
    if (detail.contained_by != null) continue;
    assert.ok(detail.selection_bounds, `${item.name} must have interaction bounds`);
    const [x,y,z] = detail.position;
    const floor = await game.raycast({ start: [x,y+2,z], end: [x,y-5,z], collision_groups: ["world"] });
    assert.ok(floor.hit_point, "supply must be above a real floor");
    const gap = detail.selection_bounds[0][1] - floor.hit_point[1];
    // Small pickup interaction boxes have a 0.2-unit minimum thickness;
    // the module's actual art is thinner. Its box can extend below the floor.
    const minimumGap = item.template_id === -938 ? -.1 : -.03;
    assert.ok(gap >= minimumGap && gap < .05, `${item.name} must rest on floor, gap=${gap}`);
  }
});
