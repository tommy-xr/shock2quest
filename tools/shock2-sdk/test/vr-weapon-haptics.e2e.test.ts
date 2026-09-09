import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const hand of ["left", "right"] as const) {
  test(`${hand} pistol cues each burst round and stays silent when empty`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const gun = await cycleToWeapon(game, e => e.template_id === -17);
    await aimVrHandAt(game, gun.position, 0.2, 1, 0, { hand });
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -0.5 : 0.5, 1, -0.6]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 10 });
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    const rounds = async () => ammoOf(await game.entities.detail(gun.id));
    const feedback = async () => (await game.info()).player.hand_feedback!.haptics!;
    const start = (await feedback()).sequence;
    const loaded = await rounds();
    assert.equal(loaded, 12);
    for (let burst = 0; burst < 4; burst++) {
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      assert.equal((await feedback()).pending[hand === "left" ? 0 : 1]!.amplitude, 0.75);
      await game.input.set(`${hand}_hand.trigger`, 0);
      await game.step({ frames: 60 });
      assert.equal(await rounds(), loaded - 3 * (burst + 1));
    }
    const expected = [...start];
    expected[hand === "left" ? 0 : 1] += loaded;
    assert.deepEqual((await feedback()).sequence, expected);
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 1 });
    assert.equal(await rounds(), 0);
    assert.deepEqual((await feedback()).sequence, expected, "dry fire produces no recoil");
    assert.deepEqual((await feedback()).pending, [null, null]);
  });
}

test("dual wielding sends recoil to the gun that fired", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const entities = (await game.entities.list()).entities;
  const pistol = entities.find(e => e.template_id === -17)!;
  const shotgun = entities.find(e => e.template_id === -19)!;
  await aimVrHandAt(game, pistol.position, 0.2, 1, 0, { hand: "left" });
  await game.step({ frames: 5 });
  await aimVrHandAt(game, shotgun.position, 0.2, 1, 0, { hand: "right" });
  await game.step({ frames: 5 });
  for (const hand of ["left", "right"] as const) {
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -0.6 : 0.6, 1, -0.6]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  }
  await game.step({ frames: 10 });
  const player = (await game.info()).player;
  assert.equal(player.wielded_entity_id, pistol.id);
  assert.equal(player.right_hand_entity_id, shotgun.id);
  const before = player.hand_feedback!.haptics!.sequence;
  const ammo = [ammoOf(await game.entities.detail(pistol.id)), ammoOf(await game.entities.detail(shotgun.id))];
  await game.input.set("left_hand.trigger", 1);
  await game.step({ frames: 1 });
  assert.deepEqual((await game.info()).player.hand_feedback!.haptics!.sequence, [before[0] + 1, before[1]]);
  assert.equal(ammoOf(await game.entities.detail(pistol.id)), ammo[0] - 1);
  assert.equal(ammoOf(await game.entities.detail(shotgun.id)), ammo[1]);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  assert.deepEqual((await game.info()).player.hand_feedback!.haptics!.sequence, before.map(n => n + 1));
  assert.equal(ammoOf(await game.entities.detail(shotgun.id)), ammo[1] - 1);
});
