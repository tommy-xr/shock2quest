import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
test("many: scriptless active Gizzards move their render/physics bodies and resume a saved path", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "many.mis" });
  await game.player.teleport({ x: 74.96556, y: 15.2442, z: 89.4715 });
  await game.camera.set({ position: [77, 30, 91], lookAt: [65, 30, 91] });
  await game.devParams.set("free_camera_cull", 1);
  const find = async (missionId: number) => {
    const entity = (await game.entities.list({ limit: 2000 })).entities.find(e => e.template_id === missionId);
    assert.ok(entity, `missing Gizzard ${missionId}`);
    return entity;
  };
  const sample = async (missionId: number) => {
    const entity = await find(missionId);
    const bodies = await game.physics.bodies({ entityId: entity.id });
    assert.equal(bodies.bodies.length, 1);
    const body = bodies.bodies[0];
    assert.equal(body.body_type, "kinematic");
    assert.equal(body.is_enabled, true);
    assert.equal(body.blocks_player, true);
    assert.ok(Math.abs(body.position[1] - entity.position[1]) < 0.02, "entity and physical pose agree");
    const rendered = (await game.scene.objects({ entityId: entity.id })).objects;
    assert.ok(rendered.length > 0, `Gizzard ${missionId} must be submitted to the renderer`);
    assert.ok(rendered.some(draw => Math.abs(draw.position[1] - entity.position[1]) < 0.02), "render and physical pose agree");
    return entity.position[1];
  };
  await game.step({ frames: 1 });
  const lower = await sample(319);
  const upper = await sample(315);
  await game.step({ frames: 60 });
  assert.ok(Math.abs((await sample(319)) - lower - 4.8) < 0.08, "lower Gizzard must rise at its authored speed");
  assert.ok(Math.abs((await sample(315)) - upper + 4.8) < 0.08, "upper Gizzard must descend at its authored speed");
  const save = `native_moving_terrain_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  await game.step({ frames: 150 });
  const uninterrupted = [await sample(319), await sample(315)];
  await game.step({ frames: 30 });
  assert.ok(Math.abs((await sample(319)) - uninterrupted[0] + 1.2) < 0.08,
    "lower Gizzard must reverse at the return edge's 2.4wu/s");
  assert.ok(Math.abs((await sample(315)) - uninterrupted[1] - 1.6) < 0.08,
    "upper Gizzard must reverse at the return edge's 3.2wu/s");
  assert.equal((await game.load(save)).success, true);
  await game.camera.set({ position: [77, 30, 91], lookAt: [65, 30, 91] });
  await game.step({ frames: 150 });
  const restored = [await sample(319), await sample(315)];
  restored.forEach((y, index) => assert.ok(Math.abs(y - uninterrupted[index]) < 0.08,
    `saved native route must resume through the turnaround: ${restored} vs ${uninterrupted}`));
});

test("many: a player standing on a native Gizzard rides its physical ascent", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "many.mis" });
  // Stage a real standing capsule just above the lower tooth's authored top.
  // No movement, flight, camera detachment or injected script message follows.
  await game.player.teleport({ x: 65.3244, y: 20.23648, z: 91.3132 });
  await game.step({ frames: 6 });
  const start = await game.player.position();
  await game.step({ frames: 60 });
  const end = await game.player.position();
  assert.ok(end.y - start.y > 4.5, `native terrain must carry the rider upward: ${JSON.stringify({ start, end })}`);
  assert.ok(Math.abs(end.y - start.y - 4.8) < 0.25, "rider and platform must share the authored speed");
  assert.ok(Math.hypot(end.x - start.x, end.z - start.z) < 0.25, "the rider must remain on the tooth");
});
