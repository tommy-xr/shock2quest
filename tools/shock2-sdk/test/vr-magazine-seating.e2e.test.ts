import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { drawPouchAmmo } from "./helpers/ammo-pouch.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

for (const [model, template] of [["atek_h", -17], ["ar15_h", -18]] as const) {
  for (const primary of ["left", "right"] as const) {
    test(`${model} ${primary}: eject removes seated mesh and real insertion restores it`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 30 });
      const gun = await cycleToWeapon(game, e => e.template_id === template);
      await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: primary, lookAtTarget: false });
      const other = primary === "left" ? "right" : "left";
      await game.input.set(`${primary}_hand.position`, [primary === "left" ? -.3 : .3, 1, -.5]);
      await game.input.set(`${primary}_hand.rotation`, [0, 0, 0, 1]);
      await game.step({ frames: 10 });
      const seated = async () => {
        const prop = (await game.entities.detail(gun.id)).properties.find(p => p.name === "MagazineSeated");
        assert.ok(prop, "the held model must expose its seated magazine");
        return JSON.parse(prop.value);
      };
      assert.deepEqual(await seated(), { present: true, rendered: true });
      const initial = ammoOf(await game.entities.detail(gun.id));
      assert.ok(initial > 0);
      const before = new Set((await game.entities.list()).entities.map(e => e.id));
      const button = primary === "left" ? "LeftHandUpperButton" : "RightHandUpperButton";
      await game.input.hold(button);
      await game.step({ frames: 45 });
      await game.input.release(button);
      await game.step({ frames: 5 });
      assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
      assert.deepEqual(await seated(), { present: false, rendered: false });
      const clips = (await game.entities.list()).entities.filter(e => !before.has(e.id));
      let ejected: number | undefined;
      for (const clip of clips) {
        const detail = await game.entities.detail(clip.id);
        if (detail.properties.some(p => p.name === "MagazineModel")) {
          ejected = clip.id;
          assert.equal(Number(detail.properties.find(p => p.name === "StackCount")?.value), initial);
        }
      }
      assert.ok(ejected != null, "ejection must produce the actual magazine with its loaded rounds");
      await game.player.spawnItem(-31);
      await game.step({ frames: 3 });
      const { offer, player: held, entityId: clip } = await drawPouchAmmo(game, other);
      const grip = held.hand_grips.find(g => g.entity_id === clip)?.grip;
      assert.ok(grip);
      const anchor = (await game.entities.detail(gun.id)).magazine_anchor;
      assert.ok(anchor);
      const local = quatRotate(quatConjugate(held.rotation), sub(anchor, held.position));
      await game.input.set(`${other}_hand.position`, sub(local, [grip.offset.x, grip.offset.y, grip.offset.z]));
      await game.step({ frames: 5 });
      assert.equal(ammoOf(await game.entities.detail(gun.id)), offer.rounds);
      assert.deepEqual(await seated(), { present: true, rendered: true });
      const final = (await game.info()).player;
      assert.equal(other === "left" ? final.wielded_entity_id : final.right_hand_entity_id, null);
      assert.ok(!(await game.entities.list()).entities.some(e => e.id === clip), "insert consumes only the real clip's rounds/entity");
    });
  }
}

test("cycling ammo after firing empty leaves the magazine seated", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
  await game.input.set("head.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 30 });
  const gun = await cycleToWeapon(game, e => e.template_id === -17);
  await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: "right", lookAtTarget: false });
  const rounds = ammoOf(await game.entities.detail(gun.id));
  for (let i = 0; i < rounds; i++) {
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 60 });
  }
  assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
  await game.input.trigger("CycleAmmo");
  await game.step({ frames: 5 });
  const seating = (await game.entities.detail(gun.id)).properties.find(p => p.name === "MagazineSeated");
  assert.ok(seating);
  assert.deepEqual(JSON.parse(seating.value), { present: true, rendered: true });
});
