import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger, muzzleFrameOf } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`PsiPull flies loot to the amp and drops it without automatic pickup (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    const [amp] = await game.entities.byTemplate(-247);
    if (vr) {
      await aimVrHandAt(game, amp.position, 0.35);
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
    }
    await selectPsiPower(game, "PsiPull");
    const [hypo] = await game.entities.byTemplate(-52);
    async function aim(id: number) {
      if (vr) await aimVrHandAt(game, (await game.entities.detail(id)).position, 5, 1);
      else await game.player.aimAt(id, { visibility: "required" });
      await game.step({ frames: 2 });
    }
    // Invalid aim must refuse before launching or spending psi.
    const [hidden] = await game.entities.byTemplate(-928);
    await game.player.teleport({ x: -13, y: 2, z: 0 });
    if (vr) await aimVrHandAt(game, (await game.entities.detail(hidden.id)).position, 5, 1);
    else await game.player.aimAt(hidden.id, { visibility: "unchecked" });
    const blockedPsi = (await game.info()).player.psi_points;
    await pullTrigger(game);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.psi_points, blockedPsi, "backstop wall blocks the pull");
    await game.player.teleport({ x: 0, y: 2, z: 0 });
    const [far] = await game.entities.byTemplate(-57);
    if (vr) await aimVrHandAt(game, (await game.entities.detail(far.id)).position, 40, 1);
    else await game.player.aimAt(far.id, { visibility: "unchecked" });
    await pullTrigger(game);
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.psi_points, blockedPsi, "distant item stays out of reach");
    await aim(hypo.id);
    const before = (await game.info()).player;
    const start = (await game.entities.detail(hypo.id)).position;
    const body = (await game.physics.bodies({ entityId: hypo.id })).bodies[0]!;
    const gravity = (await game.physics.body(body.body_id)).gravity_scale;
    assert.ok(gravity > 0);
    await pullTrigger(game);
    await game.step({ frames: 10 });
    const midFlight = (await game.entities.detail(hypo.id)).position;
    assert.ok(Math.hypot(...midFlight.map((v, i) => v - start[i]!)) > 0.1, "the actual item moves through space");
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id));
    let nearest = Infinity;
    for (let frame = 0; frame < 300; frame += 3) {
      const item = (await game.entities.detail(hypo.id)).position;
      const muzzle = muzzleFrameOf(await game.entities.detail(amp.id)).position;
      nearest = Math.min(nearest, Math.hypot(...item.map((v, i) => v - muzzle[i]!)));
      if ((await game.physics.body(body.body_id)).gravity_scale === gravity) break;
      await game.step({ frames: 3 });
    }
    assert.ok(nearest < 0.4, `item reaches the visible amp muzzle: ${nearest}`);
    assert.equal((await game.physics.body(body.body_id)).gravity_scale, gravity);
    const arrival = (await game.entities.detail(hypo.id)).position;
    await game.step({ frames: 45 });
    const dropped = (await game.entities.detail(hypo.id)).position;
    assert.ok(dropped[1] < arrival[1] - 0.05, "uncaught item falls after reaching the amp");
    const after = (await game.info()).player;
    assert.equal(after.psi_points, before.psi_points! - 1);
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id), "no automatic inventory or hand transfer, even after arrival");
    assert.ok(!after.active_psi_powers.includes("PsiPull"));
    if (vr) {
      assert.equal(after.right_hand_entity_id, amp.id);
      // This is an ordinary manual grip, not a spell transfer.
      await aimVrHandAt(game, dropped, 0.2, 0, 0, { hand: "left", lookAtTarget: false });
      await game.input.set("left_hand.squeeze", 1);
      await game.step({ frames: 8 });
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"));
      // Pull also works with both hands occupied: it never reserves a hand.
      const [clip] = await game.entities.byTemplate(-1358);
      await aim(clip.id);
      await pullTrigger(game);
      await game.step({ frames: 330 });
      assert.equal((await game.info()).player.psi_points, after.psi_points! - 1);
      assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === clip.id));
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"));
      await game.input.set("left_hand.squeeze", 0);
      await game.step({ frames: 2 });
    } else {
      assert.equal(after.wielded_entity_id, amp.id);
    }
    await game.input.set("head.look", [180, 0]);
    if (vr) await aimVrHandAt(game, [8, 3, 8], 1, 1);
    const empty = (await game.info()).player.psi_points;
    await pullTrigger(game);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.psi_points, empty, "empty aim does not spend");
  });
}

for (const vr of [false, true]) {
  for (const interruption of ["amp", "wall"] as const) {
    test(`PsiPull releases an interrupted flight (${vr ? "VR" : "flat"}, ${interruption})`, {
      skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
    }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 30 });
      const [amp] = await game.entities.byTemplate(-247);
      const [hypo] = await game.entities.byTemplate(-52);
      if (vr) {
        await aimVrHandAt(game, amp.position, 0.35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
      }
      await selectPsiPower(game, "PsiPull");
      const body = (await game.physics.bodies({ entityId: hypo.id })).bodies[0]!;
      const gravity = (await game.physics.body(body.body_id)).gravity_scale;
      if (vr) await aimVrHandAt(game, (await game.entities.detail(hypo.id)).position, 5, 1);
      else await game.player.aimAt(hypo.id, { visibility: "required" });
      const psi = (await game.info()).player.psi_points!;
      await pullTrigger(game);
      await game.step({ frames: 10 });
      assert.equal((await game.physics.body(body.body_id)).gravity_scale, 0);
      assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id));
      if (interruption === "wall") {
        // The amp moves behind the tall backstop after launch. The
        // remaining route is within range but crosses a solid wall.
        await game.player.teleport({ x: -16, y: 2, z: 0 });
      } else if (vr) {
        await game.input.set("right_hand.squeeze", 0);
      } else {
        await game.input.trigger("DebugCycleWeapon");
      }
      await game.step({ frames: 60 });
      assert.equal((await game.info()).player.psi_points, psi - 1, "a launched cast is billed once");
      assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id));
      assert.equal((await game.physics.body(body.body_id)).gravity_scale, gravity, "gravity is restored on cancellation");
      assert.ok((await game.entities.detail(hypo.id)).position[0] > -14.5, "the item never crosses the backstop");
    });
  }
}

test("PsiPull can be caught mid-flight with an ordinary VR grip", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [amp] = await game.entities.byTemplate(-247);
  const [hypo] = await game.entities.byTemplate(-52);
  await aimVrHandAt(game, amp.position, 0.35);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  await selectPsiPower(game, "PsiPull");
  await aimVrHandAt(game, (await game.entities.detail(hypo.id)).position, 5, 1);
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const body = (await game.physics.bodies({ entityId: hypo.id })).bodies[0]!;
  assert.equal((await game.physics.body(body.body_id)).gravity_scale, 0);
  await aimVrHandAt(game, (await game.entities.detail(hypo.id)).position, 0.2, 0, 0, { hand: "left", lookAtTarget: false });
  await game.input.set("left_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"));
  await game.step({ frames: 60 });
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"), "flight does not pull the caught item out of the hand");
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 30 });
  assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id));
  const released = (await game.physics.bodies({ entityId: hypo.id })).bodies[0]!;
  assert.ok((await game.physics.body(released.body_id)).gravity_scale > 0, "manual release retains ordinary gravity");
});
