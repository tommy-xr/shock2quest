import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import { quatConjugate, quatRotate, sub, add } from "./helpers/vr-hand.js";

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
