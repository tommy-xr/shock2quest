import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { HandGrip, ResolvedGrip, Vec3 } from "../src/types.js";
import { add, sub, aimVrHandAt, quatConjugate, quatMultiply, quatRotate, type Quat } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

const vector = (v: ResolvedGrip["offset"]): Vec3 => [v.x,v.y,v.z];
const quaternion = (q: ResolvedGrip["rotation"]): Quat => [...vector(q.v),q.s];
const distance = (a: Vec3,b: Vec3) => Math.hypot(...sub(a,b));
for (const [model,template] of [["fsn_h",-26],["al_h",-27],["wrench_h",-928]] as const) {
  for (const primary of ["left","right"] as const) {
    test(`${model} ${primary}: broad support locks a contact until release`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout:180_000,
    }, async () => {
      await using game = await GameServer.launch({mission:model === "wrench_h" ? "debug_interactions" : "debug_weapons",debugFlags:["--vr"]});
      await game.step({frames:30});
      const weapon = model === "wrench_h"
        ? (await game.entities.list()).entities.find(e=>e.template_id === template)!
        : await cycleToWeapon(game,e=>e.template_id === template);
      await aimVrHandAt(game,weapon.position,.2,1,0,{hand:primary});
      const other = primary === "left" ? "right" : "left";
      await game.input.set(`${primary}_hand.position`,[0,1,-.5]);
      await game.input.set(`${primary}_hand.rotation`,[0,0,0,1]);
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:30});
      const grip = async (): Promise<HandGrip> => (await game.info()).player.hand_grips.find(g=>g.hand === primary)!;
      const placeAt = async (fraction:number) => {
        const current = await grip();
        const support = current.support!;
        assert.ok(support.region_endpoints,`${model} exposes its region`);
        const [a,b] = support.region_endpoints.map(vector);
        const palm = a!.map((v,i)=>v+(b![i]!-v)*fraction) as Vec3;
        const wrist = add(vector(support.controller_position),sub(palm,vector(support.socket_position)));
        const player = (await game.info()).player;
        const inverse = quatConjugate(player.rotation);
        await game.input.set(`${other}_hand.position`,quatRotate(inverse,sub(wrist,player.position)));
        await game.input.set(`${other}_hand.rotation`,quatMultiply(inverse,quaternion(support.controller_rotation)));
      };
      assert.equal((await game.scene.fromSource("vr_support_grip")).length,0,"overlay defaults off");
      await game.devParams.set("vr_support_grips",1);
      await game.step({frames:1});
      assert.equal((await game.scene.fromSource("vr_support_grip")).length,2,"target and tracked-palm overlays render");
      await placeAt(.2);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:20});
      const attached = await grip();
      assert.equal(attached.support!.attached,true);
      const contact = vector(attached.support!.support_anchor);
      const length = distance(...attached.support!.region_endpoints!.map(vector) as [Vec3,Vec3]);
      await placeAt(.4);
      await game.step({frames:10});
      const moved = await grip();
      assert.equal(moved.support!.attached,true);
      assert.ok(distance(vector(moved.support!.tracked_palm),vector(attached.support!.tracked_palm))>length*.1,"support palm actually moves along the region");
      assert.deepEqual(vector(moved.support!.support_anchor),contact,"contact cannot slide while squeezed");
      for (const draw of (await game.scene.objects({entityId:weapon.id})).objects) {
        for (const scale of draw.scale) assert.ok(Math.abs(scale-attached.grip!.item_scale)<1e-5);
      }
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:1});
      assert.equal((await grip()).support!.attached,false);
      // Re-grab during release blend, before the prior attachment disappears.
      await placeAt(.75);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:1});
      const again = await grip();
      assert.equal(again.support!.attached,true);
      assert.ok(distance(vector(again.support!.support_anchor),contact)>length*.25,"fresh squeeze chooses a fresh contact");
      await game.devParams.set("vr_support_grips",0);
      await game.step({frames:1});
      assert.equal((await game.scene.fromSource("vr_support_grip")).length,0,"overlay can be hidden while still holding");
      await game.input.set(`${primary}_hand.squeeze`,0);
      await game.step({frames:5});
      const player = (await game.info()).player;
      assert.equal(player.wielded_entity_id,null);
      assert.equal(player.right_hand_entity_id,null);
      assert.ok(!player.hand_grips.some(g=>g.entity_id === weapon.id),"released weapon is fitted to neither hand");
    });
  }
}
