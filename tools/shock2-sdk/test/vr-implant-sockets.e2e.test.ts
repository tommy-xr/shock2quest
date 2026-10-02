import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, aimHandsAt, type Vec3 } from "../src/index.js";
import { quatConjugate, quatRotate, quatMultiply, sub, add } from "./helpers/vr-hand.js";

// Align the actual calibrated palm with the socket, keeping the receiving wrist fixed.
async function reachSocket(game: GameServer, slot: number, hand: "left" | "right", squeeze: number) {
  let player = (await game.info()).player;
  const center = player.hand_feedback?.implant_sockets?.centers[slot];
  assert.ok(center, "socket is attached to a tracked wrist");
  let local: Vec3 = quatRotate(quatConjugate(player.rotation), sub(center, player.position));
  await game.input.set(`${hand}_hand.rotation`, [1, 0, 0, 0]);
  await game.input.set(`${hand}_hand.squeeze`, squeeze);
  for (let i = 0; i < 4; i++) {
    await game.input.set(`${hand}_hand.position`, local);
    await game.step({ frames: 3 });
    player = (await game.info()).player;
    if (player.hand_feedback?.implant_sockets?.near[hand === "left" ? 0 : 1]) return;
    const palm = player.hand_feedback?.glove_contacts?.centers[hand === "left" ? 0 : 1];
    assert.ok(palm);
    local = add(local, quatRotate(quatConjugate(player.rotation), sub(center, palm)));
  }
  assert.fail("opposite palm must reach the socket");
}

for (const slot of [0, 1] as const) {
const hand = slot === 0 ? "right" : "left";
const heldField = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
test(`physical wristband implant in slot ${slot} preserves identity, energy and save state through install and removal`, {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  const spawned = await game.player.spawnItem(-101, { hand });
  const implant = { id: spawned.entity_id };
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player[heldField], implant.id);
  const prop = async (id: number, name: string) => (await game.entities.detail(id)).properties.find(p => p.name === name)?.value;
  const energy = await prop(implant.id, "Energy");
  assert.deepEqual((await game.info()).player.hand_feedback?.implant_sockets?.locked, [false, false]);
  const strength = (await game.info()).player.effective_stats!.strength;
  await reachSocket(game, slot, hand, 1);
  assert.equal(await prop(implant.id, "ImplantSlot"), undefined, "passing through a socket never equips");
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player[heldField], null);
  assert.equal(await prop(implant.id, "ImplantSlot"), String(slot));
  assert.equal((await game.info()).player.effective_stats?.strength, strength + 1);
  assert.deepEqual((await game.info()).player.hand_feedback?.implant_sockets?.locked, [slot !== 0, slot !== 1]);
  assert.equal(await prop(implant.id, "Energy"), energy);
  assert.equal((await game.physics.bodies({ entityId: implant.id })).bodies.length, 0);
  assert.equal((await game.player.inventory()).items.filter(i => i.entity_id === implant.id).length, 1);
  const save = `vr_implant_${Date.now()}`;
  assert.ok((await game.save(save)).success);
  assert.ok((await game.load(save)).success);
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.effective_stats?.strength, strength + 1, "bonus survives save/load");
  const [restored] = await game.entities.byTemplate(-101);
  assert.equal(await prop(restored.id, "ImplantSlot"), String(slot));
  assert.equal(await prop(restored.id, "Energy"), energy);
  await reachSocket(game, slot, hand, 0);
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player[heldField], restored.id);
  assert.equal(await prop(restored.id, "ImplantSlot"), undefined);
  assert.equal((await game.info()).player.effective_stats?.strength, strength);
  assert.deepEqual((await game.info()).player.hand_feedback?.implant_sockets?.locked, [false, false]);
  assert.equal(await prop(restored.id, "Energy"), energy);
});

}

test("wrist implant retrieval takes priority over a stored shoulder weapon", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await game.input.set("left_hand.squeeze", 1);
  const weapon = await game.player.spawnItem(-19, { hand: "left" });
  await game.step({ frames: 5 });
  let player = (await game.info()).player;
  const shoulder = player.hand_feedback!.shoulder_backpack!.centers![0]!;
  await game.input.set("left_hand.position", quatRotate(quatConjugate(player.rotation), sub(shoulder, player.position)));
  await game.step({ frames: 5 });
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.deepEqual((await game.info()).player.hand_feedback?.body_gear?.shoulder_weapons, [weapon.entity_id, null]);
  await game.input.set("left_hand.position", [-0.2, 0.2, -0.5]);
  await game.step({ frames: 3 });
  await game.input.set("left_hand.squeeze", 1);
  const implant = await game.player.spawnItem(-101, { hand: "left" });
  await game.step({ frames: 5 });
  await reachSocket(game, 1, "left", 1);
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.hand_feedback?.implant_sockets?.items[1], implant.entity_id);

  // A rolled, raised right wrist puts the opposite palm inside the left shoulder zone.
  player = (await game.info()).player;
  const eye = add(player.position, [0, player.camera_offset[1], 0]);
  await aimHandsAt(game, add(eye, [2, -0.4, 0]), { left: [-0.2, -0.15, -0.5], right: [0.18, -0.15, -0.35] });
  const pose = (await game.input.state()).right_hand;
  await game.input.set("right_hand.rotation", quatMultiply(quatMultiply(pose.rotation, [0, 0.5, 0, Math.sqrt(3) / 2]), [0, 0, -Math.SQRT1_2, Math.SQRT1_2]));
  await game.step({ frames: 5 });
  const center = (await game.info()).player.hand_feedback!.implant_sockets!.centers[1]!;
  await game.input.lookAtWorldPoint(center);
  await game.step({ frames: 3 });
  await reachSocket(game, 1, "left", 0);
  player = (await game.info()).player;
  const palm = player.hand_feedback!.glove_contacts!.centers[0]!;
  const backpack = player.hand_feedback!.shoulder_backpack!;
  assert.ok(Math.hypot(...sub(palm, backpack.centers![0]!)) < backpack.radius, "fixture overlaps shoulder storage");
  const strength = player.effective_stats!.strength;
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 8 });
  player = (await game.info()).player;
  assert.equal(player.wielded_entity_id, implant.entity_id, "fresh grip retrieves the implant, not the weapon");
  assert.deepEqual(player.hand_feedback?.implant_sockets?.items, [null, null]);
  assert.deepEqual(player.hand_feedback?.body_gear?.shoulder_weapons, [weapon.entity_id, null]);
  assert.equal(player.effective_stats!.strength, strength - 1);
});
