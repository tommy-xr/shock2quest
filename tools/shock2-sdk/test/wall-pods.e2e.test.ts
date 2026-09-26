import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
const enabled = process.env.SHOCK2_E2E === "1";

test("wall grub emerges through its mouth rather than above its shell", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  const [pod] = await game.entities.byTemplate(-1333);
  assert.ok(pod);
  await game.player.teleport({ x: -8, y: 1, z: 9.8 });
  await game.step({ frames: 3 });
  const [grub] = await game.entities.byTemplate(-182);
  assert.ok(grub, "natural approach must hatch the unmodified wall-grub archetype");
  assert.ok(grub.position[2]! < pod.position[2]! - 0.6, "payload starts outside the local -Z mouth");
  assert.ok(grub.position[1]! < pod.position[1]! + 0.35, "payload must not appear above the shell");
  const [body] = (await game.physics.bodies({ entityId: grub.id })).bodies;
  assert.ok(body && body.velocity[2]! < -1 && body.velocity[1]! > 1, "grub launches toward opener with lift");
  await game.step({ frames: 20 });
  const moved = await game.entities.detail(grub.id);
  assert.ok(moved.position[2]! < grub.position[2]! - 0.3, "grub escapes outward without the backing wall stopping it");
});

test("command1's yawed wall pod respects its GooEgg override and emits out of the wall", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "command1.mis" });
  await game.step({ frames: 1 });
  const [pod] = await game.entities.byTemplate(2348);
  assert.ok(pod);
  const existingClouds = new Set((await game.entities.byTemplate(-438)).map(e => e.id));
  const existingShots = new Set((await game.entities.byTemplate(-1557)).map(e => e.id));
  await game.entities.sendMessage(pod.id, { type: "TurnOn" });
  await game.step({ frames: 3 });
  const cloud = (await game.entities.byTemplate(-438)).find(e => !existingClouds.has(e.id));
  assert.ok(cloud, "this mission overrides Grub Wall Pod with GooEgg");
  assert.ok(cloud.position[2]! > pod.position[2]! + 1, "yaw180 turns local -Z mouth toward world +Z");
  assert.ok(Math.abs(cloud.position[1]! - pod.position[1]!) < 0.1);
  await game.step({ frames: 4 });
  const shot = (await game.entities.byTemplate(-1557)).find(e => !existingShots.has(e.id));
  assert.ok(shot);
  const [body] = (await game.physics.bodies({ entityId: shot.id })).bodies;
  assert.ok(body && body.velocity[2]! > 1, "goo projectile's local launch frame must face outward");
});
