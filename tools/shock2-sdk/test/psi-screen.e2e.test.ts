import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

test("Psycho-reflective Screen reduces incoming hits only while active", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  const start = (await game.info()).player;
  assert.ok(start.entity_id !== null && start.hit_points !== null);

  async function hit(): Promise<number> {
    const before = (await game.info()).player.hit_points!;
    await game.entities.sendMessage(start.entity_id!, { type: "Damage", amount: 10 });
    await game.step({ frames: 1 });
    return before - (await game.info()).player.hit_points!;
  }

  assert.equal(await hit(), 10, "an unshielded hit deals its full damage");
  await selectPsiPower(game, "Low Grav");
  const psi = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  assert.equal((await game.info()).player.psi_points, psi - 1);
  assert.ok((await game.info()).player.active_psi_powers.includes("Low Grav"));
  assert.equal(await hit(), 9, "the authored x0.85 screen rounds ten damage to nine");

  // debug_psi provisions PSI 6; Low Grav's authored duration is 20 + 30*6s.
  await game.step({ frames: 201 * 60 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Low Grav"));
  assert.equal(await hit(), 10, "damage returns to normal after expiry");
});
