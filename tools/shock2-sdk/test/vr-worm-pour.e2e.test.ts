import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import { quatMultiply, quatConjugate, quatRotate, sub } from "../src/vec.js";
import { e2ePort } from "./helpers/e2e-port.js";
import { ammoOf } from "./helpers/weapon.js";
import { stackCount } from "./helpers/nanites.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const [index, weapon] of [-27, -29].entries()) {
  test(`pouring worms into ${weapon} retains empty beakers and partial contents`, {
    skip: enabled ? false : "set SHOCK2_E2E=1 to run",
  }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons", debugFlags: ["--vr"], port: e2ePort(index),
    });
    await game.step({ frames: 30 });
    const beakerHand = index === 0 ? "left" : "right";
    const gunHand = index === 0 ? "right" : "left";
    for (const hand of [gunHand, beakerHand]) {
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    }
    await game.input.set(`${gunHand}_hand.position`, [0.25, 0.35, -0.6]);
    await game.input.set(`${beakerHand}_hand.position`, [-0.25, 0.65, -0.6]);
    const gun = (await game.player.spawnItem(weapon, { hand: gunHand })).entity_id;
    const beaker = (await game.player.spawnItem(-48, { hand: beakerHand })).entity_id;
    await game.step({ frames: 4 });
    const ammo = async () => ammoOf(await game.entities.detail(gun));
    const rounds = async (id: number) => stackCount((await game.entities.detail(id)).properties);
    assert.equal(await ammo(), 0);

    async function poseBeaker(id: number, tipped: boolean) {
      // Derive the grip's item offset instead of assuming a controller rotation
      // is also the beaker rotation. Both handed grip assets are exercised.
      await game.input.set(`${beakerHand}_hand.rotation`, [0, 0, 0, 1]);
      await game.step({ frames: 1 });
      const grip = (await game.entities.detail(id)).rotation;
      const angle = tipped ? 75 * Math.PI / 180 : 0;
      const rotation = quatMultiply([0, 0, Math.sin(angle), Math.cos(angle)], quatConjugate(grip));
      await game.input.set(`${beakerHand}_hand.rotation`, rotation);
      let hand: Vec3 = [-0.25, 0.65, -0.6];
      await game.input.set(`${beakerHand}_hand.position`, hand);
      await game.step({ frames: 1 });
      const gunPosition = (await game.entities.detail(gun)).position;
      const target: Vec3 = [gunPosition[0], gunPosition[1] + 0.4, gunPosition[2]];
      for (let i = 0; i < 3; i++) {
        const actual = (await game.entities.detail(id)).position;
        hand = hand.map((v, axis) => v + target[axis]! - actual[axis]!) as Vec3;
        await game.input.set(`${beakerHand}_hand.position`, hand);
        await game.step({ frames: 1 });
      }
    }

    await poseBeaker(beaker, false);
    await game.step({ frames: 90 });
    assert.equal(await ammo(), 0, "upright proximity must not insert the whole beaker");
    await poseBeaker(beaker, true);
    await game.step({ frames: 23 });
    assert.equal(await ammo(), 1, "one worm transfers at a time");
    assert.equal(await rounds(beaker), 3);
    assert.ok((await game.scene.fromSource("worm_pour")).length > 0, "a transferred worm renders using the grub model");
    await poseBeaker(beaker, false);
    await game.step({ frames: 90 });
    assert.equal(await ammo(), 1, "straightening pauses the pour");
    assert.equal((await game.scene.fromSource("worm_pour")).length, 0, "visual worms expire after feeding");
    await poseBeaker(beaker, true);
    await game.step({ frames: 100 });
    assert.equal(await ammo(), 4);
    const empty = (await game.entities.byTemplate(-400)).find(e => e.location === `${beakerHand}_hand`);
    assert.ok(empty && empty.id !== beaker, "the empty container remains held");
    const draws = (await game.scene.objects({ entityId: empty.id })).objects;
    assert.ok(draws.length > 0);
    assert.ok(draws.every(draw => draw.scale.every(axis => Math.abs(axis - 0.55) < 0.001)), "replacement keeps the authored held size");
    assert.equal((await game.physics.bodies({ entityId: empty.id })).bodies.length, 0, "the empty beaker has held-item physics, not a loose world body");

    // Return the actual empty container through the normal shoulder inventory
    // gesture before feeding a large beaker into the half-full gun. Only four
    // of its eight rounds may leave; staying tipped at capacity consumes none.
    const player = (await game.info()).player;
    const shoulder = player.hand_feedback?.shoulder_backpack?.centers?.[index === 0 ? 0 : 1];
    assert.ok(shoulder);
    await game.input.set(`${beakerHand}_hand.position`, quatRotate(quatConjugate(player.rotation), sub(shoulder, player.position)));
    await game.input.set(`${beakerHand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    await game.input.set(`${beakerHand}_hand.squeeze`, 0);
    await game.step({ frames: 3 });
    assert.ok((await game.player.inventory()).items.some(item => item.entity_id === empty.id && item.location === "inventory"));
    await game.input.set(`${beakerHand}_hand.position`, [-0.25, 0.65, -0.6]);
    await game.step({ frames: 2 });
    await game.input.set(`${beakerHand}_hand.squeeze`, 1);
    const large = (await game.player.spawnItem(-1264, { hand: beakerHand })).entity_id;
    await game.step({ frames: 2 });
    await poseBeaker(large, true);
    await game.step({ frames: 180 });
    assert.equal(await ammo(), 8);
    assert.equal(await rounds(large), 4, "remaining worms stay in the same beaker");
    await game.step({ frames: 90 });
    assert.equal(await rounds(large), 4);

    // Move the same partially used large beaker to an empty gun, exhausting
    // it into the large empty container rather than the small variant.
    await game.input.set(`${gunHand}_hand.squeeze`, 0);
    await game.step({ frames: 1 });
    await game.input.set(`${gunHand}_hand.squeeze`, 1);
    const nextGun = (await game.player.spawnItem(weapon, { hand: gunHand })).entity_id;
    await game.step({ frames: 120 });
    assert.equal(ammoOf(await game.entities.detail(nextGun)), 4);
    const largeEmpty = (await game.entities.byTemplate(-487)).find(e => e.location === `${beakerHand}_hand`);
    assert.ok(largeEmpty, "the large beaker retains its own empty variant");
  });
}
