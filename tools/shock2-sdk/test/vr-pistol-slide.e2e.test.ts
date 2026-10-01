import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) {
  test(`VR pistol ${hand}: slide cycles, locks open empty, and closes on reload`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.input.set("head.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    const gun = await cycleToWeapon(game, e => e.template_id === -17);
    await aimVrHandAt(game, gun.position, .2, 1, 0, { hand, lookAtTarget: false });
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.3 : .3, 1, -.5]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 20 });
    // Measure the rendered slide relative to the receiver in weapon space,
    // so tracked hand movement/whole-weapon recoil cannot pass this assertion.
    // The mounted remaster orders the slide at joint 2; its scalar parameter is 0.
    const slidePosition = async () => {
      const pose = await game.entities.animation(gun.id);
      assert.ok(pose);
      return quatRotate(quatConjugate(pose.rotation), sub(pose.joints[2], pose.joints[0]));
    };
    const closed = await slidePosition();
    const travel = async () => Math.hypot(...sub(await slidePosition(), closed));
    const pull = async () => {
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      await game.step({ frames: 1 });
    };
    const initial = ammoOf(await game.entities.detail(gun.id));
    assert.ok(initial > 1);
    await pull();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial - 1);
    const openTravel = await travel();
    assert.ok(openTravel > .025 && openTravel < .1, `slide must retract, travel=${openTravel}`);
    await game.step({ frames: 60 });
    assert.ok(await travel() < .001, "loaded pistol returns to battery");
    for (let i = 1; i < initial; i++) {
      await pull();
      await game.step({ frames: 60 });
    }
    assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
    assert.ok(Math.abs(await travel() - openTravel) < .002, "last round holds slide open");
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 1 });
    assert.ok(Math.abs(await travel() - openTravel) < .002, "dry fire must not start a fresh cycle");
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.step({ frames: 60 });
    assert.ok(Math.abs(await travel() - openTravel) < .002, "dry fire stays open");
    await game.player.spawnItem(-31);
    await game.step({ frames: 3 });
    await game.input.trigger("Reload");
    await game.step({ frames: 120 });
    assert.ok(ammoOf(await game.entities.detail(gun.id)) > 0);
    assert.ok(await travel() < .001, "successful reload releases the slide");
  });
}
