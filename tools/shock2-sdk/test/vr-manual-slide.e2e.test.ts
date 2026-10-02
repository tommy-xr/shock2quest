import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, setHandWorldPose, attachSupportHand } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, pullTrigger } from "./helpers/weapon.js";

for (const [primary, physical] of [["left", false], ["right", false], ["right", true]] as const) {
  test(`manual pistol slide ${primary}${physical ? " physical" : ""}: rail, release, ownership and support`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: physical ? ["--vr", "--experimental", "physical_held_items"] : ["--vr"] });
    await game.input.set("head.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 30 });
    const gun = await cycleToWeapon(game, e => e.template_id === -17);
    await aimVrHandAt(game, gun.position, .2, 1, 0, { hand: primary, lookAtTarget: false });
    const other = primary === "left" ? "right" : "left";
    await game.input.set(`${primary}_hand.position`, [0, 1, -.5]);
    await game.input.set(`${primary}_hand.rotation`, [0, 0, 0, 1]);
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 20 });
    const grip = async () => (await game.info()).player.hand_grips.find(g => g.entity_id === gun.id)!;
    const slide = (await grip()).slide;
    assert.ok(slide, "the held Nightdive pistol must expose a slide contact");
    const p = slide.controller_position, t = slide.world_travel, q = slide.controller_rotation;
    const place = async (fraction: number, sideways = 0) => {
      await setHandWorldPose(game, (await game.info()).player, other,
        [p.x + t.x * fraction, p.y + t.y * fraction + sideways, p.z + t.z * fraction],
        [q.v.x, q.v.y, q.v.z, q.s]);
    };
    const joint = async () => {
      const pose = await game.entities.animation(gun.id);
      assert.ok(pose);
      return quatRotate(quatConjugate(pose.rotation), sub(pose.joints[2], pose.joints[0]));
    };
    const closed = await joint();
    const travel = async () => Math.hypot(...sub(await joint(), closed));
    const initial = ammoOf(await game.entities.detail(gun.id));
    await place(0);
    await game.step({ frames: 2 });
    await game.input.set(`${other}_hand.squeeze`, 1);
    await game.step({ frames: 2 });
    assert.equal((await grip()).slide?.attached, true);
    assert.equal((await grip()).support?.attached, false);
    const receiver = (await game.entities.animation(gun.id))!;
    const gloveStart = (await grip()).slide!.glove_pose!.position;
    for (const fraction of [.5, 1, .25, 1]) {
      await place(fraction);
      await game.step({ frames: 1 });
      const contact = (await grip()).slide!;
      assert.ok(Math.abs(contact.fraction - fraction) < .025);
      const glove = contact.glove_pose!.position;
      assert.ok(Math.hypot(glove.x - gloveStart.x - t.x * fraction, glove.y - gloveStart.y - t.y * fraction,
        glove.z - gloveStart.z - t.z * fraction) < .002, "glove follows the rendered part in the same frame");
      assert.ok(Math.abs(await travel() - Math.hypot(t.x, t.y, t.z) * fraction) < .002);
      const pose = (await game.entities.animation(gun.id))!;
      assert.ok(Math.hypot(...sub(pose.joints[0], receiver.joints[0])) < .002, "pulling must not translate the receiver");
      assert.ok(Math.abs(pose.rotation.reduce((sum, x, i) => sum + x * receiver.rotation[i], 0)) > .9999, "pulling must not steer the pistol");
    }
    assert.equal(ammoOf(await game.entities.detail(gun.id)), initial, "manual motion is cosmetic in this increment");
    const player = (await game.info()).player;
    assert.equal(primary === "left" ? player.wielded_entity_id : player.right_hand_entity_id, gun.id);
    assert.equal(other === "left" ? player.wielded_entity_id : player.right_hand_entity_id, null);
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 1 });
    const releasing = await travel();
    assert.ok(releasing > .01, "release must spring forward over time");
    await game.step({ frames: 30 });
    assert.ok(await travel() < .001);
    await place(0);
    await game.input.set(`${other}_hand.squeeze`, 1);
    await game.step({ frames: 2 });
    assert.equal((await grip()).slide?.attached, true);
    await place(0, .25);
    await game.step({ frames: 2 });
    assert.equal((await grip()).slide?.attached, false, "leaving the rail releases the contact");
    await place(0);
    await game.step({ frames: 5 });
    assert.equal((await grip()).slide?.attached, false, "held grip cannot reacquire after a break");
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 10 });
    await attachSupportHand(game, primary);
    assert.equal((await grip()).support?.attached, true, "normal pistol support remains available");
    assert.equal((await grip()).slide?.attached, false);
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 30 });
    for (let shot = 0; shot < initial; shot++) {
      await pullTrigger(game, primary);
      await game.step({ frames: 60 });
    }
    assert.equal(ammoOf(await game.entities.detail(gun.id)), 0, "manual manipulation adds no firing gate");
    assert.ok((await grip()).slide!.fraction > .99, "empty locks open");
    await place(1);
    await game.step({ frames: 2 });
    await game.input.set(`${other}_hand.squeeze`, 1);
    await game.step({ frames: 2 });
    assert.equal((await grip()).slide?.attached, true, "grab the currently open slide");
    await place(0);
    await game.step({ frames: 3 });
    assert.ok((await grip()).slide!.fraction > .99, "manual motion preserves the empty lock");
    await game.input.set(`${other}_hand.squeeze`, 0);
    await game.step({ frames: 30 });
    assert.ok((await grip()).slide!.fraction > .99);
    await game.player.spawnItem(-31);
    await game.step({ frames: 3 });
    await game.input.trigger("Reload");
    await game.step({ frames: 120 });
    assert.ok(ammoOf(await game.entities.detail(gun.id)) > 0);
    assert.ok((await grip()).slide!.fraction < .01, "normal reload still releases the lock");
  });
}
