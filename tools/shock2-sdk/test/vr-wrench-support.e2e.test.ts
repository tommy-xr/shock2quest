import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { HandGrip, ResolvedGrip, Vec3 } from "../src/types.js";
import { add, sub, aimVrHandAt, quatConjugate, quatMultiply, quatRotate, type Quat } from "./helpers/vr-hand.js";

const vector = (v: ResolvedGrip["offset"]): Vec3 => [v.x, v.y, v.z];
const quaternion = (q: ResolvedGrip["rotation"]): Quat => [...vector(q.v), q.s];
const distance = (a: Vec3, b: Vec3) => Math.hypot(...sub(a,b));

for (const primary of ["right", "left"] as const) {
  test(`wrench support (${primary} primary): near grab, rigid steering, release and separation break`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({mission:"debug_interactions", debugFlags:["--vr"]});
      await game.input.set("head.rotation",[0,0,0,1]);
      await game.step({frames:90});
      const wrench = (await game.entities.list()).entities.find(e => e.template_id === -928)!;
      const other = primary === "right" ? "left" : "right";
      const owned = primary === "right" ? "right_hand_entity_id" : "wielded_entity_id";
      const empty = primary === "right" ? "wielded_entity_id" : "right_hand_entity_id";
      await game.player.teleport({x:wrench.position[0],y:1,z:0});
      // Keep the head facing forward: the pickup target is off to the side,
      // and turning toward it puts the support fixture in a shoulder bag.
      await aimVrHandAt(game, wrench.position, .2, 1, 0, {hand:primary, lookAtTarget:false});
      await game.input.set(`${primary}_hand.position`, [0,1,-.5]);
      await game.input.set(`${primary}_hand.rotation`, [0,0,0,1]);
      await game.input.set(`${other}_hand.squeeze`, 0);
      await game.step({frames:90});
      const grip = async (): Promise<HandGrip> => (await game.info()).player.hand_grips.find(g => g.hand === primary)!;
      const before = await grip();
      assert.ok(before?.support, "wrench publishes its prepared support socket");
      const itemScale = before.grip!.item_scale;
      const place = async (position: Vec3, rotation = quaternion(before.support!.controller_rotation)) => {
        const player = (await game.info()).player;
        const inverse = quatConjugate(player.rotation);
        await game.input.set(`${other}_hand.position`, quatRotate(inverse, sub(position, player.position)));
        await game.input.set(`${other}_hand.rotation`, quatMultiply(inverse, rotation));
      };
      const socketHand = vector(before.support.controller_position);
      // A squeeze begun away from the handle cannot attach by drifting into it.
      await place(add(socketHand,[0,0,.6]));
      await game.input.set(`${other}_hand.squeeze`, 1);
      await game.step({frames:3});
      await place(socketHand);
      await game.step({frames:3});
      assert.equal((await grip()).support!.attached,false);
      await game.input.set(`${other}_hand.squeeze`, 0);
      await game.step({frames:2});
      await game.input.set(`${other}_hand.squeeze`, 1);
      await game.step({frames:15});
      assert.equal((await grip()).support!.attached,true, JSON.stringify({before:before.support, support:(await grip()).support}));
      const state = (await game.info()).player;
      assert.equal(state[owned],wrench.id);
      assert.equal(state[empty],null,"support does not own a second entity");
      assert.equal(state.hand_grips.length,1);
      // Move only the supporting controller sideways; scale and the primary palm stay fixed.
      await place(add(socketHand,[.09,0,0]));
      await game.input.set(`${other}_hand.trigger`,1);
      await game.step({frames:45});
      const steered = await grip();
      assert.equal(steered.support!.attached,true);
      assert.equal((await game.info()).player[empty],null,"support trigger cannot acquire another item");
      assert.ok(distance(vector(steered.support!.primary_palm),vector(before.support.primary_palm)) < .0001);
      assert.ok(distance(vector(steered.support!.socket_position),vector(before.support.socket_position)) > .04);
      const q = quaternion(steered.support!.model_rotation);
      const root = vector(steered.support!.model_position);
      const palm = add(root,quatRotate(q,vector(steered.support!.primary_anchor)));
      assert.ok(distance(palm,vector(before.support.primary_palm)) < .0001,"model keeps primary anchor exact");
      for (const draw of (await game.scene.objects({entityId:wrench.id})).objects) {
        for (const scale of draw.scale) assert.ok(Math.abs(scale-itemScale) < 1e-5,"weapon never stretches");
      }
      await game.input.set(`${other}_hand.trigger`,0);
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:1});
      const releasing = await grip();
      assert.equal(releasing.support!.attached,false);
      assert.ok(releasing.support!.blend > 0,"support release blends instead of snapping");
      assert.equal((await game.info()).player[owned],wrench.id);
      await game.step({frames:60});
      assert.ok(distance(vector((await grip()).support!.model_position),vector(before.support.model_position)) < .001);
      // Over-separation ends support; moving back while squeezed does not reattach.
      await place(socketHand);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:10});
      assert.equal((await grip()).support!.attached,true);
      await place(add(socketHand,[0,.5,0]));
      await game.step({frames:1});
      assert.equal((await grip()).support!.attached,false);
      await place(socketHand);
      await game.step({frames:10});
      assert.equal((await grip()).support!.attached,false);
      await game.input.set(`${other}_hand.squeeze`,0);
      await game.step({frames:60});
      await place(socketHand);
      await game.input.set(`${other}_hand.squeeze`,1);
      await game.step({frames:10});
      assert.equal((await grip()).support!.attached,true);
      await game.input.set(`${primary}_hand.squeeze`,0);
      await game.step({frames:5});
      const released = (await game.info()).player;
      assert.equal(released[owned],null);
      assert.equal(released[empty],null,"primary release never hands the wrench to the supporting hand");
      assert.equal(released.hand_grips.length,0);
    });
}


test("physical wrench glove stays attached during walking and wall contact", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({mission:"debug_melee",debugFlags:["--vr"]});
  await game.input.set("head.rotation",[0,0,0,1]);
  await game.step({frames:90});
  const wrench = (await game.entities.list()).entities.find(e => e.template_id === -928)!;
  await aimVrHandAt(game,wrench.position,.2,1,0,{lookAtTarget:false});
  await game.player.teleport({x:-11,y:1,z:0});
  // Hold in front of the torso while sweeping into the wall along X.
  await game.input.set("right_hand.position",[0,1,-.5]);
  await game.input.set("right_hand.rotation",[0,0,0,1]);
  await game.step({frames:90});
  const check = async () => {
    const g = (await game.info()).player.hand_grips.find(g=>g.hand === "right")!;
    assert.ok(g.glove_pose && g.support && g.grip);
    const modelFromGlove = add(vector(g.glove_pose.position),quatRotate(quaternion(g.glove_pose.rotation),vector(g.grip.offset)));
    assert.ok(distance(modelFromGlove,vector(g.support.model_position)) < .0001,"glove and rendered physical weapon agree every frame");
    return g;
  };
  await game.input.set("right_hand.thumbstick",[.7,0]);
  for (let frame=0;frame<20;frame++) { await game.step({frames:1}); await check(); }
  await game.input.set("right_hand.thumbstick",[0,0]);
  await game.player.teleport({x:-11,y:1,z:0});
  await game.step({frames:30});
  // The debug_melee back wall is x=-13. Sweep the held controller through it.
  for (let frame=0;frame<30;frame++) {
    await game.input.set("right_hand.position",[-3*frame/29,1,-.5]);
    await game.step({frames:1});
    await check();
  }
  await game.step({frames:30});
  const blocked = await check();
  const player = (await game.info()).player;
  const trackedModel = add(player.position,quatRotate(player.rotation,add([-3,1,0],vector(blocked.grip!.offset))));
  assert.ok(distance(trackedModel,vector(blocked.support!.model_position)) > .5,"wall actually blocks the weapon away from the tracked target");
  assert.ok(vector(blocked.support!.model_position)[0] > -13,"weapon remains on near side of wall");
  // The controller is beyond the wall, but both visible glove and wrench stop
  // on this side. A second hand on that visible socket must still attach.
  await game.input.set("left_hand.squeeze",0);
  const visiblePalm = add(vector(blocked.support!.model_position),quatRotate(quaternion(blocked.support!.model_rotation),vector(blocked.support!.primary_anchor)));
  assert.ok(distance(visiblePalm,vector(blocked.support!.primary_palm))>.5,"visible primary palm is displaced from its controller");
  const inverse = quatConjugate(player.rotation);
  await game.input.set("left_hand.position",quatRotate(inverse,sub(vector(blocked.support!.controller_position),player.position)));
  await game.input.set("left_hand.rotation",quatMultiply(inverse,quaternion(blocked.support!.controller_rotation)));
  await game.step({frames:1});
  await game.input.set("left_hand.squeeze",1);
  await game.step({frames:10});
  const supported = await check();
  assert.equal(supported.support!.attached,true,"second hand can grab the visible wrench while its primary controller is blocked: "+JSON.stringify({before:blocked.support,after:supported.support}));
  assert.equal((await game.info()).player.right_hand_entity_id,wrench.id);
  assert.equal((await game.info()).player.hand_grips.length,1,"support retains one owner");
  await game.input.set("left_hand.position",quatRotate(inverse,sub(add(vector(blocked.support!.controller_position),[.025,0,.025]),player.position)));
  for (let frame=0;frame<8;frame++) {
    await game.step({frames:1});
    const moving = await check();
    assert.equal(moving.support!.attached,true,"physical contact feedback is not a support-release gesture");
  }
  await game.input.set("right_hand.position",[-2.98,1,-.5]);
  await game.step({frames:8});
  const shifted = await check();
  assert.equal(shifted.support!.attached,true);
  const primaryMotion = sub(vector(shifted.support!.primary_palm),vector(supported.support!.primary_palm));
  assert.ok(Math.hypot(...primaryMotion)>.01,"primary controller moved");
  assert.ok(distance(vector(shifted.support!.control_primary_palm),add(vector(supported.support!.control_primary_palm),primaryMotion))<.0001,"control pivot follows the primary palm translation exactly, including player motion");
  await game.input.set("left_hand.squeeze",0);
  await game.step({frames:1});
  assert.equal((await check()).support!.attached,false,"explicit release still ends support");
});
