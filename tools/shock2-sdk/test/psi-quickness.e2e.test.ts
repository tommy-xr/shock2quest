import assert from "node:assert/strict";
import { test, type TestContext } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { launchWithSettings } from "./helpers/settings.js";
import { worldSeconds } from "./helpers/world-clock.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Short backward walks stay on the clear floor behind the psi pen. Avoid
// teleporting with a held amp so hand and weapon stay together throughout.
async function travel(game: GameServer, vr: boolean): Promise<number> {
  await game.input.set(vr ? "left_hand.thumbstick" : "right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 1 });
  await game.input.set(vr ? "left_hand.thumbstick" : "right_hand.thumbstick", [0, -0.2]);
  await game.step({ frames: 1 });
  const before = (await game.info()).player.position;
  await game.step({ frames: 30 });
  await game.input.set(vr ? "left_hand.thumbstick" : "right_hand.thumbstick", [0, 0]);
  const after = (await game.info()).player.position;
  return Math.hypot(after[0] - before[0], after[2] - before[2]);
}

async function launchPsiPen(t: TestContext, mode: "SlowWorld" | "Classic", vr: boolean): Promise<GameServer> {
  const game = await launchWithSettings(t, { vr: { speed_boosts: mode } }, {
    mission: "debug_psi", debugFlags: vr ? ["--vr"] : [],
  });
  await game.step({ frames: 30 });
  if (vr) {
    const [amp] = await game.entities.byTemplate(-247);
    await aimVrHandAt(game, amp.position, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
  }
  return game;
}

for (const vr of [false, true]) {
  test(`Classic Quickness increases effective Agility and actual movement, refreshes and expires (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async (t) => {
    await using game = await launchPsiPen(t, "Classic", vr);
    await game.input.set("head.look", [0, 0]);
    const before = (await game.info()).player;
    const baseline = await travel(game, vr);
    await selectPsiPower(game, "Quickness");
    await pullTrigger(game);
    let player = (await game.info()).player;
    assert.equal(player.psi_points, before.psi_points! - 1);
    assert.ok(player.active_psi_powers.includes("Quickness"));
    assert.equal(player.stats!.agility, 6, "training is unchanged");
    assert.equal(player.effective_stats!.agility, 8, "authored bonus adds two effective levels");
    const faster = await travel(game, vr);
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
    assert.ok(Math.abs((await travel(game, vr)) / baseline - 1) < 0.005, "expiry restores the trained movement speed");
  });
}

// Slow world (default): Quickness is bullet time instead of +2 Agility, slower
// the higher the PSI, and lasts its duration on the player's clock.
test("Slow world Quickness slows the world instead of raising Agility", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async (t) => {
  await using game = await launchPsiPen(t, "SlowWorld", false);
  await game.input.set("head.look", [0, 0]);
  const before = (await game.info()).player;
  const baseline = await travel(game, false);
  await selectPsiPower(game, "Quickness");
  await pullTrigger(game);
  const player = (await game.info()).player;
  assert.ok(player.active_psi_powers.includes("Quickness"));
  assert.equal(player.effective_stats!.agility, before.effective_stats!.agility, "no Agility bonus");
  const psi = player.effective_stats!.psionic_ability;
  const expected = Math.max(0.25, 0.5 - 0.05 * Math.max(0, psi - 3));
  assert.ok(Math.abs((await worldSeconds(game, 60)) - expected) < 1e-3, `world scale at PSI ${psi}`);
  assert.ok(Math.abs((await travel(game, false)) / baseline - 1) < 0.005, "player speed unchanged");

  // Authored duration (120 s + 60 s per PSI), counted in player frames.
  const duration = 120 + 60 * psi;
  await game.step({ frames: (duration - 4) * 60 });
  assert.ok((await game.info()).player.active_psi_powers.includes("Quickness"), "still active");
  await game.step({ frames: 4 * 60 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Quickness"), "expired on time");
  assert.ok(Math.abs((await worldSeconds(game, 60)) - 1) < 1e-3, "the world returns to full speed");
});
