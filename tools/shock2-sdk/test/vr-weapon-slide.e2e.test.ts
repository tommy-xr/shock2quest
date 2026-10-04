import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, pullTrigger } from "./helpers/weapon.js";
import { unlockWeaponAlternateFire } from "./helpers/weapon-upgrades.js";

for (const [weapon, template, locksEmpty] of [["pistol", -17, true], ["AR15", -18, false]] as const) {
  for (const hand of ["left", "right"] as const) {
    test(`VR ${weapon} ${hand}: slide cycles and handles empty/reload`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.step({ frames: 30 });
      const gun = await cycleToWeapon(game, e => e.template_id === template);
      await aimVrHandAt(game, gun.position, .2, 1, 0, { hand, lookAtTarget: false });
      await game.step({ frames: 5 });
      if (weapon === "AR15") await unlockWeaponAlternateFire(game);
      await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.3 : .3, 1, -.5]);
      await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
      await game.step({ frames: 20 });
      // Measure the rendered slide relative to the receiver in weapon space,
      // so tracked hand movement/whole-weapon recoil cannot pass this assertion.
      let magazineRest: Vec3 | undefined;
      const slidePosition = async () => {
        const pose = await game.entities.animation(gun.id);
        assert.ok(pose);
        if (weapon === "AR15") {
          const magazine = quatRotate(quatConjugate(pose.rotation), sub(pose.joints[2], pose.joints[0]));
          magazineRest ??= magazine;
          assert.ok(Math.hypot(...sub(magazine, magazineRest)) < .001, "firing must not move the magazine");
        }
        return quatRotate(quatConjugate(pose.rotation), sub(pose.joints[weapon === "pistol" ? 2 : 1], pose.joints[0]));
      };
      const closed = await slidePosition();
      const travel = async () => Math.hypot(...sub(await slidePosition(), closed));
      const pull = () => pullTrigger(game, hand);
      const initial = ammoOf(await game.entities.detail(gun.id));
      assert.ok(initial > 1);
      await pull();
      assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - 1);
      const openTravel = await travel();
      assert.ok(openTravel > .025 && openTravel < .1, `slide must retract, travel=${openTravel}`);
      await game.step({ frames: 60 });
      assert.ok(await travel() < .001, "loaded weapon returns to battery");
      if (weapon === "AR15") {
        await game.input.trigger("CycleGunSetting");
        await game.step({ frames: 2 });
        const beforeAuto = ammoOf(await game.entities.detail(gun.id));
        await game.input.set(`${hand}_hand.trigger`, 1);
        let retractions = 0;
        let wasBack = false;
        for (let frame = 0; frame < 30; frame++) {
          await game.step({ frames: 1 });
          const back = await travel() > openTravel * .95;
          if (back && !wasBack) retractions++;
          wasBack = back;
        }
        await game.input.set(`${hand}_hand.trigger`, 0);
        await game.step({ frames: 60 });
        assert.ok(beforeAuto - ammoOf(await game.entities.detail(gun.id)) >= 4);
        assert.ok(retractions >= 4, `AUTO must animate successive shots, got ${retractions} retractions`);
        assert.ok(await travel() < .001, "releasing AUTO lets the handle return forward");
        await game.input.trigger("CycleGunSetting");
        await game.step({ frames: 2 });
      }
      for (let i = 0; i < initial && ammoOf(await game.entities.detail(gun.id)) > 0; i++) {
        await pull();
        await game.step({ frames: 60 });
      }
      assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
      assert.ok(Math.abs(await travel() - (locksEmpty ? openTravel : 0)) < .002, "empty pose is weapon-specific");
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      assert.ok(Math.abs(await travel() - (locksEmpty ? openTravel : 0)) < .002, "dry fire must not start a fresh cycle");
      await game.input.set(`${hand}_hand.trigger`, 0);
      await game.step({ frames: 60 });
      assert.ok(Math.abs(await travel() - (locksEmpty ? openTravel : 0)) < .002, "dry fire preserves the empty pose");
      await game.player.spawnItem(-31);
      await game.step({ frames: 3 });
      await game.input.trigger("Reload");
      await game.step({ frames: 120 });
      assert.ok(ammoOf(await game.entities.detail(gun.id)) > 0);
      assert.ok(await travel() < .001, "successful reload releases the slide");
    });
  }
}
