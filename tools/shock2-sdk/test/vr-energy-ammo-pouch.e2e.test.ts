import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { drawPouchAmmo } from "./helpers/ammo-pouch.js";
import { ammoOf } from "./helpers/weapon.js";
import { stackCount } from "./helpers/nanites.js";

for (const [name, weapon, ammo, hand] of [
  ["laser battery", -22, -3641, "left"],
  ["EMP battery", -23, -3641, "right"],
  ["fusion prism", -26, -41, "left"],
  ["stasis prism", -25, -44, "right"],
  ["worm launcher beaker", -27, -48, "left"],
  ["viral proliferator beaker", -29, -1264, "right"],
] as const) {
  test(`${name}: pouch draws stocked ammo and returns it intact`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    const primary = hand === "left" ? "right" : "left";
    await game.input.set(`${primary}_hand.position`, [primary === "left" ? -.3 : .3, 1, -.5]);
    await game.input.set(`${primary}_hand.squeeze`, 1);
    const gun = await game.player.spawnItem(weapon, { hand: primary });
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.hand_feedback?.body_gear?.pouch.state, "empty");
    const reserve = await game.player.spawnItem(ammo);
    if (weapon !== -23) await game.player.spawnItem(ammo);
    await game.step({ frames: 5 });
    const rounds = async (id: number) => stackCount((await game.entities.detail(id)).properties)!;
    const stock = await rounds(reserve.entity_id);
    const loaded = ammoOf(await game.entities.detail(gun.entity_id));
    const readout = (await game.info()).player.hand_feedback?.body_gear?.pouch;
    assert.equal(readout?.state, "ready");
    assert.equal(readout?.count, stock);
    if (ammo === -3641) assert.equal(readout?.icon, "i_batt");
    const { offer, entityId } = await drawPouchAmmo(game, hand);
    assert.equal(offer.reserve, reserve.entity_id);
    if (ammo === -3641) assert.equal(offer.rounds, 1, "draw one battery, not a magazine-sized stack");
    assert.equal(await rounds(entityId), offer.rounds);
    if (stock === offer.rounds) {
      assert.equal(entityId, reserve.entity_id, "last stock moves the existing entity");
      assert.equal((await game.info()).player.hand_feedback?.body_gear?.pouch.count, 0);
    } else {
      assert.notEqual(entityId, reserve.entity_id);
      assert.equal(await rounds(reserve.entity_id), stock - offer.rounds);
    }
    await game.step({ frames: 20 });
    assert.equal(hand === "left" ? (await game.info()).player.wielded_entity_id : (await game.info()).player.right_hand_entity_id, entityId, "held squeeze does not draw twice");
    assert.equal((await game.entities.detail(entityId)).template_id, ammo);
    assert.equal(ammoOf(await game.entities.detail(gun.entity_id)), loaded, "drawing does not load or use ammo");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 8 });
    const player = (await game.info()).player;
    assert.equal(hand === "left" ? player.wielded_entity_id : player.right_hand_entity_id, null);
    assert.equal(await rounds(reserve.entity_id), stock, "return preserves the complete reserve");
    assert.equal((await game.player.inventory()).items.find(i => i.entity_id === reserve.entity_id)?.location, "inventory");
  });
}
