import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("a hatched grub animates, leaves its pod and pursues upright", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  await game.player.teleport({ x: -6, y: 1, z: 0 });
  await game.step({ frames: 30 });
  const [grub] = await game.entities.byTemplate(-182);
  assert.ok(grub, "approaching the pod must hatch a grub");
  const start = await game.entities.detail(grub.id);
  const segments = start.aim_points ?? [];
  assert.equal(segments.length, 4, "head, chest, abdomen and tail must have damage proxies");
  for (const segment of segments) {
    const body = await game.physics.body(segment.body_id);
    assert.ok(body.collision_groups.includes("hitbox"));
    assert.equal(body.blocks_actor, false, "damage proxies must not push the actor");
    const hit = await game.raycast({
      start: [segment.position[0]!, segment.position[1]! + 0.5, segment.position[2]!],
      end: segment.position,
      collision_groups: ["hitbox"],
    });
    assert.ok(segments.some(p => p.proxy_entity_id === hit.entity_id), "segment geometry must be ray-hittable");
  }
  const before = await game.entities.animation(grub.id);
  assert.ok(before, "the object model must have a joint pose");
  await game.step({ frames: 10 });
  const after = await game.entities.animation(grub.id);
  assert.ok(after);
  const relative = (pose: typeof before) => pose.joints.slice(0, 4).map(joint =>
    joint.map((value, axis) => value - pose.position[axis]!));
  assert.notDeepEqual(relative(before), relative(after), "joint tweqs must animate without motion clips");
  assert.equal(after.clip, null, "a grub does not use skeletal locomotion clips");

  let moved = false;
  for (let i = 0; i < 24; i++) {
    await game.step({ frames: 10 });
    const [body] = (await game.physics.bodies({ entityId: grub.id })).bodies;
    assert.ok(body);
    assert.equal(body.body_type, "dynamic");
    assert.ok(body.angular_velocity.every(v => Math.abs(v) < 0.001), "live grub must not tumble");
    assert.ok(Math.abs(body.rotation[0]!) < 0.001 && Math.abs(body.rotation[2]!) < 0.001,
      "facing changes must preserve upright pitch and roll");
    assert.ok(body.position[1]! < 4, "controlled hop must stay near its target's height");
    moved ||= Math.hypot(body.position[0]! - start.position[0]!, body.position[2]! - start.position[2]!) > 0.5;
  }
  assert.ok(moved, "the grub must leave its pod and pursue without an injected alert or impulse");
  const current = await game.entities.detail(grub.id);
  const proxy = current.aim_points?.[0];
  assert.ok(proxy);
  await game.entities.sendMessage(proxy.proxy_entity_id, { type: "Damage", amount: 5 });
  await game.step({ frames: 3 });
  assert.equal((await game.entities.byTemplate(-182)).length, 0, "death must remove the live actor");
  for (const segment of segments) {
    assert.equal((await game.physics.bodies({ entityId: segment.proxy_entity_id })).bodies.length, 0,
      "death must remove the joint collision proxies");
  }
  assert.equal((await game.entities.byTemplate(-2666)).length, 1, "death must use the authored grub flinders");
});


test("a launched grub settles with its visible model above the floor", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  const [pod] = await game.entities.byTemplate(-1335);
  assert.ok(pod);
  await game.entities.sendMessage(pod.id, { type: "TurnOn" });
  await game.step({ frames: 30 });
  const [grub] = await game.entities.byTemplate(-182);
  assert.ok(grub);
  // Hold calm to isolate physical landing from a new attack at the player.
  await game.entities.sendMessage(grub.id, { type: "SetAlertness", level: "Lowest" });
  await game.step({ frames: 1 });
  const [body] = (await game.physics.bodies({ entityId: grub.id })).bodies;
  assert.ok(body && body.mass);
  const response = await fetch(`${game.baseUrl}/v1/physics/bodies/${body.body_id}/impulse`, {
    method: "POST", body: JSON.stringify({ impulse: [0, body.mass * 0.9, body.mass * 3] }),
  });
  assert.equal(response.ok, true);
  await game.step({ frames: 180 });
  const [landed] = (await game.physics.bodies({ entityId: grub.id })).bodies;
  assert.ok(landed);
  assert.ok(Math.abs(landed.velocity[1]!) < 0.05, "the actor should have landed");
  // This station's kerb is at y=.1; grub3's lower extent is -.052.
  assert.ok(landed.position[1]! > 0.14 && landed.position[1]! < 0.17,
    `the model must rest on the kerb, not below it: ${landed.position}`);
  assert.ok(landed.angular_velocity.every(v => Math.abs(v) < 0.001));
});
