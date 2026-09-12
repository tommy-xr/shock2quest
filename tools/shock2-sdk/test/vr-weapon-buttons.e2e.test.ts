import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { stackCount } from "./helpers/nanites.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";


for (const hand of ["left", "right"] as const) {
  test(`${hand} upper button swaps carried clips with exact identity and rounds`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const gunHand = hand === "left" ? "right" : "left";
    const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
    const button = hand === "left" ? "LeftHandUpperButton" : "RightHandUpperButton";
    const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17);
    assert.ok(pistol);
    await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: gunHand });
    await game.step({ frames: 5 });
    const standard = await game.player.spawnItem(-31);
    const highExplosive = await game.player.spawnItem(-32);
    const unrelated = await game.player.spawnItem(-42);
    const original = [standard, highExplosive, unrelated];
    const rounds = await Promise.all(original.map(async item => stackCount((await game.entities.detail(item.entity_id)).properties)));
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    await game.step({ frames: 5 });
    const player = (await game.info()).player;
    const center = player.hand_feedback?.ammo_pouch?.center;
    assert.ok(center);
    await game.input.set(`${hand}_hand.position`, quatRotate(quatConjugate(player.rotation), sub(center, player.position)));
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 3 });
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player[owner], standard.entity_id);
    // Leave the body pouch and the gun's insertion zone before changing type.
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -1.4 : 1.4, 1.5, -0.5]);
    await game.step({ frames: 5 });
    if (hand === "right") {
      let full = false;
      for (let i = 0; i < 50; i++) {
        try { await game.player.spawnItem(-1221); }
        catch { full = true; break; }
      }
      assert.ok(full, "exercise a full backpack");
    }
    await game.input.hold(button);
    await game.step({ frames: 45 });
    assert.equal((await game.info()).player[owner], highExplosive.entity_id, "holding ammo cycles only once");
    await game.input.release(button);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player[owner], highExplosive.entity_id, "release does not cycle a second time");
    let inventory = (await game.player.inventory()).items;
    assert.equal(inventory.find(i => i.entity_id === standard.entity_id)?.location, "inventory");
    assert.equal(inventory.find(i => i.entity_id === highExplosive.entity_id)?.location, `${hand}_hand`);
    assert.equal(ammoOf(await game.entities.detail(pistol.id)), loaded);
    for (let i = 0; i < original.length; i++) assert.equal(stackCount((await game.entities.detail(original[i].entity_id)).properties), rounds[i]);
    assert.equal((await game.physics.bodies({ entityId: standard.entity_id })).bodies.length, 0, "stored clip has no physics");
    // Let go of the gun. Ammo families must still work when no gun is carried.
    await game.input.set(`${gunHand}_hand.squeeze`, 0);
    await game.step({ frames: 5 });
    await game.input.trigger(button);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player[owner], standard.entity_id);
    inventory = (await game.player.inventory()).items;
    assert.equal(inventory.find(i => i.entity_id === highExplosive.entity_id)?.location, "inventory");
    for (let i = 0; i < original.length; i++) assert.equal(stackCount((await game.entities.detail(original[i].entity_id)).properties), rounds[i]);
  });
}

test("gun tap fires on release and simultaneous grip release cancels it", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const pistol = (await game.entities.list()).entities.find(e => e.template_id === -17);
  assert.ok(pistol);
  await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: "left" });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, pistol.id);
  const initial = (await game.info()).player.wielded_gun_setting;
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.hold("LeftHandUpperButton");
  await game.step({ frames: 12 });
  assert.equal((await game.info()).player.wielded_gun_setting, initial, "press alone does not change mode");
  await game.input.release("LeftHandUpperButton");
  await game.step({ frames: 2 });
  const changed = (await game.info()).player.wielded_gun_setting;
  assert.notEqual(changed, initial);
  assert.ok((await game.audio.recent()).sounds.some(s => s.sequence > before && s.sample.toLowerCase().startsWith("bset")));
  await game.input.hold("LeftHandUpperButton");
  await game.step({ frames: 12 });
  await game.input.set("left_hand.squeeze", 0);
  await game.input.release("LeftHandUpperButton");
  await game.step({ frames: 3 });
  assert.notEqual((await game.info()).player.wielded_entity_id, pistol.id);
  const dropped = (await game.entities.list()).entities.find(e => e.id === pistol.id);
  assert.ok(dropped);
  await aimVrHandAt(game, dropped.position, 0.2, 1, 0, { hand: "left" });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, pistol.id);
  assert.equal((await game.info()).player.wielded_gun_setting, changed, "dropped gun did not receive the tap");
  await game.input.hold("LeftHandUpperButton");
  await game.step({ frames: 12 });
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 3 });
  await game.input.trigger("TogglePauseMenu");
  await game.step({ frames: 40 });
  await game.input.release("LeftHandUpperButton");
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.wielded_gun_setting, changed, "pause canceled the tap");
  assert.ok(ammoOf(await game.entities.detail(pistol.id)) > 0, "resuming a held button did not eject");
});
