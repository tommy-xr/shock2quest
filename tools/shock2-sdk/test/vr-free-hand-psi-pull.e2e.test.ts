import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const ampHand of ["left", "right"] as const) {
  test(`trained free-hand pull preserves the ${ampHand} amp selection and charges once`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const [amp] = await game.entities.byTemplate(-247);
    const freeHand = ampHand === "left" ? "right" : "left";
    await aimVrHandAt(game, amp.position, 0.3, 1, 0, { hand: ampHand });
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[ampHand === "left" ? "wielded_entity_id" : "right_hand_entity_id"], amp.id);
    const selected = (await game.info()).player.selected_psi_power;
    assert.notEqual(selected, "PsiPull");
    const [item] = await game.entities.byTemplate(-52);
    await aimVrHandAt(game, item.position, 3, 0, 0, { hand: freeHand });
    await game.step({ frames: 3 });
    const before = (await game.info()).player.psi_points!;
    // Default-off negative case: the empty trigger does not spend psi.
    await game.input.set(`${freeHand}_hand.trigger`, 1);
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.psi_points, before);
    await game.devParams.set("vr_free_hand_psi_pull", 1);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.psi_points, before, "enabling while pressed cannot cast");
    await game.input.set(`${freeHand}_hand.trigger`, 0);
    await game.step({ frames: 2 });
    const start = (await game.entities.detail(item.id)).position;
    await aimVrHandAt(game, start, 3, 0, 1, { hand: freeHand });
    await game.step({ frames: 20 });
    const moved = (await game.entities.detail(item.id)).position;
    assert.ok(Math.hypot(...moved.map((v, i) => v - start[i]!)) > 0.15, "pull moves the original item");
    assert.equal((await game.info()).player.psi_points, before - 1);
    await game.step({ frames: 120 });
    assert.equal((await game.info()).player.psi_points, before - 1, "held trigger cannot repeat the cast");
    assert.equal((await game.info()).player.selected_psi_power, selected);
    assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === item.id), "pull never duplicates or auto-stores loot");
  });
}
