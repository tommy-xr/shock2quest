import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Short backward walks stay on the clear floor behind the psi pen. Avoid
// teleporting with a held amp so hand and weapon stay together throughout.
async function travel(game: GameServer): Promise<number> {
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.thumbstick", [0, -0.2]);
  await game.step({ frames: 1 });
  const before = (await game.info()).player.position;
  await game.step({ frames: 30 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  const after = (await game.info()).player.position;
  return Math.hypot(after[0] - before[0], after[2] - before[2]);
}

for (const vr of [false, true]) {
  test(`Quickness increases effective Agility and actual movement, refreshes and expires (${vr ? "VR" : "flat"})`, {
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
    await game.input.set("head.look", [0, 0]);
    const before = (await game.info()).player;
    const baseline = await travel(game);
    await selectPsiPower(game, "Quickness");
    await pullTrigger(game);
    let player = (await game.info()).player;
    assert.equal(player.psi_points, before.psi_points! - 1);
    assert.ok(player.active_psi_powers.includes("Quickness"));
    assert.equal(player.stats!.agility, 6, "training is unchanged");
    assert.equal(player.effective_stats!.agility, 8, "authored bonus adds two effective levels");
    const faster = await travel(game);
    assert.ok(Math.abs(faster / baseline - 2 / 1.7) < 0.005, `authored speed ratio: ${baseline} → ${faster}`);
    await pullTrigger(game);
    player = (await game.info()).player;
    assert.equal(player.psi_points, before.psi_points! - 2);
    assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:quickness").length, 1);
    assert.equal(player.effective_stats!.agility, 8, "recasting refreshes rather than stacking");

    // Real missions support save/load; carry the active power out of the pen.
    await game.transitionLevel("earth.mis");
    const save = `e2e_psi_quickness_${vr}_${Date.now()}`;
    await game.save(save);
    await game.load(save);
    player = (await game.info()).player;
    assert.equal(player.stats!.agility, 6);
    assert.equal(player.effective_stats!.agility, 8);
    assert.ok(player.active_psi_powers.includes("Quickness"));
    const remaining = player.stats!.modifiers.find(m => m.source === "psi:quickness")!.remaining;
    await game.step({ frames: (remaining.secs + 2) * 60 });
    player = (await game.info()).player;
    assert.equal(player.effective_stats!.agility, 6);
    assert.ok(!player.active_psi_powers.includes("Quickness"));
    assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:quickness").length, 0);
    await game.player.teleport({ x: 0, y: 100, z: 0 });
    assert.ok(Math.abs((await travel(game)) / baseline - 1) < 0.005, "expiry restores the trained movement speed");
  });
}
