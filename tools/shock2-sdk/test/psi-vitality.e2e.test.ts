import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`Psychogenic Endurance grants its authored temporary +2 END (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    if (vr) {
      const [amp] = await game.entities.byTemplate(-247);
      await aimVrHandAt(game, amp.position, 0.35);
      await game.input.set("right_hand.squeeze", 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
    }
    const initial = (await game.info()).player;
    await game.entities.sendMessage(initial.entity_id!, { type: "Damage", amount: 7 });
    await game.step({ frames: 1 });
    const before = (await game.info()).player;
    assert.equal(before.hit_points, initial.hit_points! - 7);
    await selectPsiPower(game, "Vitality");
    await pullTrigger(game);
    const active = (await game.info()).player;
    assert.equal(active.psi_points, before.psi_points! - 4);
    assert.ok(active.active_psi_powers.includes("Vitality"));
    assert.equal(active.stats!.endurance, before.stats!.endurance, "the trained level stays unchanged");
    assert.equal(active.effective_stats!.endurance, before.effective_stats!.endurance + 2);
    assert.ok(active.max_hit_points! > before.max_hit_points!);
    assert.equal(active.max_hit_points! - active.hit_points!, before.max_hit_points! - before.hit_points!);
    const modifier = active.stats!.modifiers.find(m => m.source === "psi:vitality")!;
    assert.equal(modifier.remaining.secs, 479, "authored duration is 120 + 60 × effective PSI (6)");
    await game.step({ frames: 478 * 60 });
    assert.ok((await game.info()).player.active_psi_powers.includes("Vitality"));
    await game.step({ frames: 3 * 60 });
    const expired = (await game.info()).player;
    assert.equal(expired.effective_stats!.endurance, before.effective_stats!.endurance);
    assert.equal(expired.max_hit_points, before.max_hit_points);
    assert.equal(expired.hit_points, before.hit_points);
    assert.deepEqual(expired.active_psi_powers, []);
    assert.equal(expired.stats!.modifiers.filter(m => m.source === "psi:vitality").length, 0);
  });
}

test("Vitality refreshes once, survives level transition and save/load, then expires", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  await selectPsiPower(game, "Vitality");
  await pullTrigger(game);
  await game.step({ frames: 60 * 60 });
  const spent = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  let player = (await game.info()).player;
  assert.equal(player.psi_points, spent - 4);
  assert.deepEqual(player.active_psi_powers, ["Vitality"]);
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:vitality").length, 1);
  assert.equal(player.effective_stats!.endurance, 8);

  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  assert.equal((await game.info()).player.effective_stats!.endurance, 8);
  const saveName = `e2e_psi_vitality_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  await game.step({ frames: 10 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.endurance, 8);
  assert.deepEqual(player.active_psi_powers, ["Vitality"]);
  const buffedMaximum = player.max_hit_points!;
  await game.entities.sendMessage(player.entity_id!, { type: "Damage", amount: player.hit_points! - 2 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.hit_points, 2);
  await game.step({ frames: 481 * 60 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.endurance, player.stats!.endurance);
  assert.deepEqual(player.active_psi_powers, []);
  assert.ok(player.max_hit_points! < buffedMaximum);
  assert.equal(player.hit_points, 1, "losing temporary max HP cannot kill a living player");
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:vitality").length, 0);
});
