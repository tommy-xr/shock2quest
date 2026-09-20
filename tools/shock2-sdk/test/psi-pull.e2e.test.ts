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
    await pullTrigger(game);
    await game.step({ frames: 5 });
    const after = (await game.info()).player;
    assert.equal(after.psi_points, before.psi_points! - 1);
    assert.ok(!after.active_psi_powers.includes("PsiPull"), "Pull is instant despite its gamesys activation type");
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
