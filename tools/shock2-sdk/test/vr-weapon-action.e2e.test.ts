import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, pullTrigger } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) {
  test(`VR fusion ${hand}: core and holder rotate independently on accepted shots`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { heavy_weapons: 6 } });
    const gun = await cycleToWeapon(game, e => e.template_id === -26, { settleFrames: 90 });
    await aimVrHandAt(game, gun.position, .45, 1, 0, { hand, lookAtTarget: false });
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.35 : .35, 1.35, -.65]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    const pose = async () => {
      const p = await game.entities.animation(gun.id);
      assert.ok(p);
      return p;
    };
    assert.equal(hand === "left" ? (await game.info()).player.wielded_entity_id : (await game.info()).player.right_hand_entity_id, gun.id);
    const rest = await pose();
    const pull = () => pullTrigger(game, hand);
    const before = ammoOf(await game.entities.detail(gun.id));
    await pull();
    assert.equal(ammoOf(await game.entities.detail(gun.id)), before - 2);
    const moving = await pose();
    // The rotational pivots can be stationary: inspect actual posed basis
    // vectors, not just joint positions or the driver's requested parameters.
    assert.ok(moving.joint_axes, "runtime must expose posed joint axes");
    const delta = (a: number[][], b: number[][]) => Math.hypot(...a.flat().map((v, i) => v - b.flat()[i]));
    assert.ok(delta(moving.joint_axes[1], rest.joint_axes[1]) > .1, "core must rotate");
    assert.ok(delta(moving.joint_axes[2], rest.joint_axes[2]) > .1, "holder must rotate");
    assert.ok(delta(moving.joint_axes[0], rest.joint_axes[0]) < .0001, "receiver stays at the tracked grip");
    await game.step({ frames: 65 });
    const settled = await pose();
    for (const joint of [1, 2]) assert.ok(delta(settled.joint_axes[joint], rest.joint_axes[joint]) < .0001);
    // Empty firing must leave the action at rest.
    await game.input.trigger("EjectClip");
    await game.step({ frames: 3 });
    assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
    await pull();
    const dry = await pose();
    for (const joint of [1, 2]) assert.ok(delta(dry.joint_axes[joint], rest.joint_axes[joint]) < .0001);
  });
}
