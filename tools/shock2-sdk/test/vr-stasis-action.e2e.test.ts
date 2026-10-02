import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, pullTrigger } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) {
  test(`VR stasis ${hand}: firing and successful reload cycle the cylinder`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { heavy_weapons: 6 } });
    const gun = await cycleToWeapon(game, e => e.template_id === -25, { settleFrames: 90 });
    await aimVrHandAt(game, gun.position, .45, 1, 0, { hand, lookAtTarget: false });
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.35 : .35, 1.35, -.65]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    assert.equal(hand === "left" ? (await game.info()).player.wielded_entity_id : (await game.info()).player.right_hand_entity_id, gun.id);
    await game.player.spawnItem(-44);
    await game.input.trigger("Reload");
    await game.step({ frames: 180 });
    const pose = async () => {
      const p = await game.entities.animation(gun.id); assert.ok(p); return p;
    };
    const rest = await pose();
    const local = (p: typeof rest, j: number) => quatRotate(quatConjugate(p.rotation), sub(p.joints[j], p.joints[0]));
    const travel = (p: typeof rest, j: number) => Math.hypot(...sub(local(p, j), local(rest, j)));
    const axisChange = (p: typeof rest, j: number) => Math.hypot(...p.joint_axes[j].flat().map((v, i) => v - rest.joint_axes[j].flat()[i]));
    const before = ammoOf(await game.entities.detail(gun.id));
    await pullTrigger(game, hand);
    assert.ok(ammoOf(await game.entities.detail(gun.id)) < before);
    const kick = await pose();
    assert.ok(travel(kick, 3) > .005, "small action retracts on the shot");
    assert.ok(axisChange(kick, 2) > .1, "cylinder inherits its parent rotation");
    assert.ok(axisChange(kick, 0) < .0001, "receiver remains fixed");
    await game.step({ frames: 29 });
    assert.ok(travel(await pose(), 2) > .05, "cylinder opens after the shot");
    await game.step({ frames: 60 });
    const settled = await pose();
    for (const j of [2, 3]) assert.ok(travel(settled, j) < .001);
    assert.ok(axisChange(settled, 2) < .0001);
    await game.input.trigger("EjectClip"); await game.step({ frames: 3 });
    assert.equal(ammoOf(await game.entities.detail(gun.id)), 0);
    await pullTrigger(game, hand); await game.step({ frames: 30 });
    assert.ok(travel(await pose(), 2) < .001, "dry firing must not cycle the cylinder");
    await game.player.spawnItem(-44);
    await game.input.trigger("Reload"); await game.step({ frames: 2 });
    assert.ok(ammoOf(await game.entities.detail(gun.id)) > 0);
    assert.ok(travel(await pose(), 2) > .05, "successful reload opens the cylinder");
    await game.step({ frames: 180 });
    assert.ok(travel(await pose(), 2) < .001);
    await game.input.trigger("Reload"); await game.step({ frames: 2 });
    assert.ok(travel(await pose(), 2) < .001, "a full magazine reload must not start a cosmetic cycle");
  });
}
