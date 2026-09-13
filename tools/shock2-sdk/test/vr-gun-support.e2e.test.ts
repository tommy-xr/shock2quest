import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { HandGrip, ResolvedGrip, Vec3 } from "../src/types.js";
import { add, sub, aimVrHandAt, quatConjugate, quatMultiply, quatRotate, type Quat } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

const vector = (v: ResolvedGrip["offset"]): Vec3 => [v.x,v.y,v.z];
const quaternion = (q: ResolvedGrip["rotation"]): Quat => [...vector(q.v),q.s];
const distance = (a: Vec3,b: Vec3) => Math.hypot(...sub(a,b));

for (const [model, template] of [["atek_h",-17],["ar15_h",-18],["sg_h",-19]] as const) {
  for (const primary of ["left","right"] as const) {
    test(`${model} support (${primary}): steering, firing, scale, ownership and release`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({mission:"debug_weapons",debugFlags:model === "ar15_h" ? ["--vr","--experimental","physical_held_items"] : ["--vr"]});
      await game.step({frames:30});
      const weapon = await cycleToWeapon(game,e=>e.template_id === template);
      await aimVrHandAt(game,weapon.position,.2,1,0,{hand:primary});
      const other = primary === "left" ? "right" : "left";
      const owner = primary === "left" ? "wielded_entity_id" : "right_hand_entity_id";
      const empty = primary === "left" ? "right_hand_entity_id" : "wielded_entity_id";
      // Keep support acquisition clear of the enlarged body-inventory regions.
      await game.input.set(`${primary}_hand.position`,[0,1,-1.2]);
      await game.input.set(`${primary}_hand.rotation`,[0,0,0,1]);
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:30});
      const grip = async (): Promise<HandGrip> => (await game.info()).player.hand_grips.find(g=>g.hand === primary)!;
      const before = await grip();
      const initialBody = model === "ar15_h"
        ? (await game.physics.bodies({entityId:weapon.id})).bodies[0] : null;
      if (model === "ar15_h") {
        assert.ok(initialBody,"AR support must retain the physical held body");
        assert.deepEqual(initialBody.collision_groups,[],"held gun contacts stay inert");
      }
      if (initialBody) assert.equal(initialBody.body_type,"kinematic");
      assert.ok(before?.support && before.grip,`${model} publishes its authored support socket`);
      const socket = vector(before.support.controller_position);
      const place = async (worldPosition: Vec3) => {
        const player = (await game.info()).player;
        const inverse = quatConjugate(player.rotation);
        await game.input.set(`${other}_hand.position`,quatRotate(inverse,sub(worldPosition,player.position)));
        await game.input.set(`${other}_hand.rotation`,quatMultiply(inverse,quaternion(before.support!.controller_rotation)));
      };
      await place(socket);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:15});
      assert.equal((await grip()).support!.attached,true);
      if (initialBody) {
        const body = (await game.physics.bodies({entityId:weapon.id})).bodies[0]!;
        const forward = quatRotate(body.rotation,[-1,0,0]);
        const original = quatRotate(initialBody.rotation,[-1,0,0]);
        assert.ok(forward.reduce((sum,v,i)=>sum+v*original[i]!,0) > .99,
          "support attachment preserves the primary barrel direction");
      }
      await place(add(socket,[.07,0,0]));
      await game.step({frames:30});
      const steered = await grip();
      assert.equal(steered.support!.attached,true);
      assert.ok(distance(vector(steered.support!.socket_position),vector(before.support.socket_position)) > .01);
      assert.ok(distance(vector(steered.support!.primary_palm),vector(before.support.primary_palm)) < 1e-4);
      assert.ok(steered.glove_pose,"primary glove follows the supported weapon");
      const fromGlove = add(vector(steered.glove_pose.position),quatRotate(quaternion(steered.glove_pose.rotation),vector(steered.grip!.offset)));
      assert.ok(distance(fromGlove,vector(steered.support!.model_position)) < 1e-4);
      for (const draw of (await game.scene.objects({entityId:weapon.id})).objects) {
        for (const scale of draw.scale) assert.ok(Math.abs(scale-before.grip.item_scale) < 1e-5);
      }
      const ammo = ammoOf(await game.entities.detail(weapon.id));
      assert.ok(ammo > 0);
      const pulses = async () => (await game.info()).player.hand_feedback!.haptics!;
      const beforePulses = (await pulses()).sequence;
      await game.input.set(`${other}_hand.trigger`,1);
      await game.step({frames:2});
      assert.equal(ammoOf(await game.entities.detail(weapon.id)),ammo,"support trigger cannot fire");
      assert.deepEqual((await pulses()).sequence, beforePulses);
      assert.equal((await grip()).support!.visual_trigger,1,"reserved support input remains available to finger animation");
      assert.equal((await game.info()).player[empty],null);
      // Pistol/shotgun rounds are fast raycast projectiles, not Rapier bodies.
      // Barrel geometry is checked by the weapon-script solver integration test.
      await game.input.set(`${primary}_hand.trigger`,1);
      await game.step({frames:1});
      assert.equal(ammoOf(await game.entities.detail(weapon.id)),ammo-1,"primary trigger fires while supported");
      const feedback = await pulses();
      assert.deepEqual(feedback.sequence, beforePulses.map(n => n + 1));
      const fired = await grip();
      assert.equal(fired.support!.attached,true,"support latch survives actual firing");
      assert.equal(fired.visual_trigger,1);
      assert.deepEqual(fired.finger_curls,fired.grip!.trigger_curls ?? fired.grip!.curls);
      await game.input.set(`${primary}_hand.trigger`,0);
      if (model === "ar15_h") {
        await game.step({frames:12});
        assert.equal((await grip()).support!.attached,true,"support stays latched through physical recoil");
      }
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:1});
      assert.equal((await grip()).support!.attached,false);
      assert.equal((await game.info()).player[owner],weapon.id);
      assert.equal((await game.info()).player[empty],null);
      await game.input.set(`${other}_hand.trigger`,0);
      await game.step({frames:model === "ar15_h" ? 180 : 60});
      assert.ok(distance(vector((await grip()).support!.model_position),vector(before.support.model_position)) < .001);
      await place(socket);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:15});
      assert.equal((await grip()).support!.attached,true);
      await game.input.set(`${primary}_hand.squeeze`,0);
      await game.step({frames:5});
      const released = (await game.info()).player;
      assert.equal(released[owner],null);
      assert.equal(released[empty],null,"primary release drops instead of transferring ownership");
    });
  }
}
