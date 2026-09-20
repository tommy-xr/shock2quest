import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`PsiPull acquires remote loose loot and refuses blocked targets (${vr ? "VR" : "flat"})`, {
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
    // Both negative cases occur before receiving a held item, so an occupied
    // VR hand cannot accidentally make them pass.
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
    await pullTrigger(game);
    await game.step({ frames: 10 });
    const midFlight = (await game.entities.detail(hypo.id)).position;
    assert.ok(Math.hypot(...midFlight.map((v, i) => v - start[i]!)) > 0.1, "the actual item moves through space");
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id), "no transfer before arrival");
    await game.step({ frames: 180 });
    const after = (await game.info()).player;
    assert.equal(after.psi_points, before.psi_points! - 1);
    assert.ok(!after.active_psi_powers.includes("PsiPull"), "flight is a one-shot cast, not a sustained buff");
    if (vr) {
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"), "remote item is held in the receiving hand");
      assert.equal(after.right_hand_entity_id, amp.id, "retain the casting amp");
      const [clip] = await game.entities.byTemplate(-1358);
      await aim(clip.id);
      await pullTrigger(game);
      await game.step({ frames: 5 });
      assert.equal((await game.info()).player.psi_points, after.psi_points, "occupied receiving hand refuses without spending");
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"));
      await game.input.set("left_hand.squeeze", 1);
      await game.step({ frames: 2 });
      await game.input.set("left_hand.squeeze", 0);
      await game.step({ frames: 2 });
      assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === hypo.id && i.location === "left_hand"), "squeeze then release drops the received item");
    } else {
      assert.ok((await game.player.inventory()).items.some(i => i.entity_id === hypo.id));
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
        // The receiver moves behind the tall backstop after launch. The
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
