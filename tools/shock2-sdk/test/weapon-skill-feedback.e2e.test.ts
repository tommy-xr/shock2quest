import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { ammoOf, pullTrigger, cycleToWeapon, fireOnce } from "./helpers/weapon.js";

test("Earth weapon training survives save/load without granting permanent skills", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 30 });
  await game.player.spawnItem("Pistol");
  await game.input.trigger("EquipPistol");
  await game.step({ frames: 5 });
  const ammo = async () => {
    const id = (await game.info()).player.wielded_entity_id;
    assert.ok(id);
    return ammoOf(await game.entities.detail(id));
  };
  const before = await ammo();
  assert.ok(before > 1);
  await fireOnce(game);
  assert.equal(await ammo(), before - 1, "Earth's authored allowance permits firing");
  const saveName = `weapon_training_${Date.now()}`;
  assert.equal((await game.save(saveName)).success, true);
  assert.equal((await game.load(saveName)).success, true);
  await fireOnce(game);
  assert.equal(await ammo(), before - 2, "saved-position loads reconstruct the allowance");
  assert.equal((await game.info()).player.stats?.skills.standard_weapons, 0);
  assert.equal((await game.info()).player.stats?.skills.energy_weapons, 0);
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 5 });
  const afterTransition = await ammo();
  await fireOnce(game);
  assert.equal(await ammo(), afterTransition, "tutorial allowance does not leave Earth");
  assert.ok((await game.ui.state()).messages.includes(
    "Requires Standard Weapons 1 - You have 0",
  ));
});

test("an under-skilled rifle attempt explains its requirement and training unlocks firing", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  // A real mission retains its low character sheet; debug_weapons deliberately
  // caps every skill, which would never exercise the rejection.
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 5 });
  await game.player.setStats({ skills: { standard_weapons: 4 } });
  const rifle = await game.player.spawnItem("Assault Rifle");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, rifle.entity_id);
  const ammo = async () => ammoOf(await game.entities.detail(rifle.entity_id));
  const before = await ammo();
  assert.ok(before > 0, "the rifle must have ammunition to test the skill gate");
  const expected = "Requires Standard Weapons 6 - You have 4";
  await pullTrigger(game);
  assert.equal(await ammo(), before, "a rejected shot consumes no ammunition");
  assert.ok((await game.ui.state()).messages.includes(expected));
  assert.equal((await game.entities.detail(rifle.entity_id)).properties.find(
    p => p.name === "WeaponSkillNotice",
  )?.value, expected);

  await pullTrigger(game);
  assert.equal((await game.ui.state()).messages.filter(m => m === expected).length, 1,
    "repeated attempts must not stack identical messages");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 190 });
  assert.equal(await ammo(), before, "holding trigger does not bypass the gate");
  assert.ok(!(await game.entities.detail(rifle.entity_id)).properties.some(
    p => p.name === "WeaponSkillNotice",
  ), "the notice expires while trigger stays held");
  assert.ok(!(await game.ui.state()).messages.includes(expected),
    "flat text expires with the weapon notice, without stale duplicate lines");
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 1 });

  await pullTrigger(game);
  assert.equal((await game.ui.state()).messages.filter(m => m === expected).length, 1);

  await game.player.setStats({ skills: { standard_weapons: 6 } });
  assert.ok(!(await game.ui.state()).messages.includes(expected),
    "training immediately removes the obsolete requirement");
  await pullTrigger(game);
  assert.equal(await ammo(), before - 1, "the authored threshold allows a real shot");
});

test("a trigger refusal is visible for a weapon with no ammo readout", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 5 });
  const shard = await game.player.spawnItem("Crystal Shard");
  await game.input.trigger("EquipCrystalShard");
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.wielded_entity_id, shard.entity_id);
  await pullTrigger(game);
  assert.ok((await game.ui.state()).messages.includes(
    "Requires Exotic Weapons 1 - You have 0",
  ));
});

for (const hand of ["left", "right"] as const) {
  test(`VR ${hand} hand: rejected shots explain the requirement and expire without firing`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { standard_weapons: 4 } });
    const rifle = await cycleToWeapon(game, e => e.name === "Assault Rifle", { settleFrames: 90 });
    await aimVrHandAt(game, rifle.position, 0.45, 1, 0, { hand });
    await game.step({ frames: 8 });
    const player = (await game.info()).player;
    assert.equal(hand === "right" ? player.right_hand_entity_id : player.wielded_entity_id, rifle.id);
    assert.equal(hand === "right" ? player.wielded_entity_id : player.right_hand_entity_id, null);
    const ammo = async () => ammoOf(await game.entities.detail(rifle.id));
    const notice = async () => (await game.entities.detail(rifle.id)).properties.find(
      p => p.name === "WeaponSkillNotice",
    )?.value;
    const before = await ammo();
    assert.ok(before > 0);
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 1 });
    assert.equal(await ammo(), before);
    assert.equal(await notice(), "Requires Standard Weapons 6 - You have 4");
    await game.step({ frames: 190 });
    assert.equal(await notice(), undefined, "a held trigger must let the notice expire");
    assert.equal(await ammo(), before, "a held trigger must not bypass the skill gate");
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.step({ frames: 1 });
    await game.player.setStats({ skills: { standard_weapons: 6 } });
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 1 });
    assert.equal(await ammo(), before - 1, "the authored threshold permits firing in either hand");
    assert.equal(await notice(), undefined);
  });
}
