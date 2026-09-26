import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
const enabled = process.env.SHOCK2_E2E === "1";

test("hatched swarm flies, backs off, damages nearby player and expires", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  const health = (await game.info()).player.hit_points!;
  await game.player.teleport({ x: -6, y: 1, z: 6 });
  await game.step({ frames: 15 });
  const [swarm] = await game.entities.byTemplate(-183);
  assert.ok(swarm, "approaching the swarmer pod must hatch its cloud");
  const start = (await game.entities.detail(swarm.id)).position;
  let moved = false;
  const behaviors = new Set<string>();
  for (let i = 0; i < 36; i++) {
    await game.step({ frames: 10 });
    const detail = await game.entities.detail(swarm.id);
    const [body] = (await game.physics.bodies({ entityId: swarm.id })).bodies;
    assert.ok(body);
    assert.equal(body.body_type, "dynamic");
    assert.ok(body.position[1]! > 0.3 && body.position[1]! < 4, `cloud must hover: ${body.position}`);
    moved ||= Math.hypot(detail.position[0]! - start[0]!, detail.position[2]! - start[2]!) > 0.7;
    for (const prop of detail.properties) if (prop.name === "AIBehavior") behaviors.add(prop.value);
  }
  assert.ok(moved, "the cloud must leave its pod");
  assert.ok([...behaviors].some(v => v.includes("BackOff")), `swarm must retreat between approaches: ${[...behaviors]}`);
  assert.ok((await game.info()).player.hit_points! < health, "authored proximity stim must reach the player");
  // The Swarm script's twenty-second lifetime runs independently of AI combat.
  await game.player.teleport({ x: 20, y: 1, z: 6 });
  await game.step({ frames: 850 });
  assert.equal((await game.entities.byTemplate(-183)).length, 0, "expired swarm must disappear");
  assert.equal((await game.physics.bodies({ entityId: swarm.id })).bodies.length, 0);
});

test("rec1 swarm flight and remaining lifetime survive save/load", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "rec1.mis" });
  await game.step({ frames: 1 });
  const [pod] = await game.entities.byTemplate(264);
  assert.ok(pod, "discover the authored swarmer pod by mission object id");
  await game.entities.sendMessage(pod.id, { type: "TurnOn" });
  await game.step({ frames: 5 });
  const [swarm] = await game.entities.byTemplate(-183);
  assert.ok(swarm);
  await game.entities.sendMessage(swarm.id, { type: "SetAlertness", level: "Lowest" });
  await game.step({ frames: 295 });
  const before = await game.entities.detail(swarm.id);
  const slot = `swarmer_lifetime_${Date.now()}`;
  assert.equal((await game.save(slot)).success, true);
  assert.equal((await game.load(slot)).success, true);
  const [restored] = await game.entities.byTemplate(-183);
  assert.ok(restored);
  const [body] = (await game.physics.bodies({ entityId: restored.id })).bodies;
  assert.ok(body && body.body_type === "dynamic", "restore the flying body");
  assert.ok(Math.hypot(...body.position.map((v, i) => v - before.position[i]!)) < 0.1,
    "loading should preserve the cloud position");
  await game.step({ frames: 780 });
  assert.equal((await game.entities.byTemplate(-183)).length, 1, "saved swarm should live until its remaining deadline");
  await game.step({ frames: 180 });
  assert.equal((await game.entities.byTemplate(-183)).length, 0, "loading must not restart the twenty-second clock");
});
