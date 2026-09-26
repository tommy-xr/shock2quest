import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };

async function cast(game: GameServer) {
  await selectPsiPower(game, "Inviso");
  const before = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  const player = (await game.info()).player;
  assert.equal(player.psi_points, before - 4);
  assert.ok(player.active_psi_powers.includes("Inviso"));
}
// debug_psi provisions PSI 6; setStats only raises stats.
for (const psi of [6]) {
  test(`Invisibility duration follows character PSI ${psi}`, options, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: psi });
    await cast(game);
    await game.step({ frames: (5 + 5 * psi - 2) * 60 });
    assert.ok((await game.info()).player.active_psi_powers.includes("Inviso"));
    await game.step({ frames: 3 * 60 });
    assert.ok(!(await game.info()).player.active_psi_powers.includes("Inviso"));
  });
}
for (const weapon of ["Pistol", "Wrench"] as const) {
  test(`${weapon} attack reveals the invisible player`, options, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });
    await game.step({ frames: 10 });
    await cast(game);
    await game.player.spawnItem(weapon === "Pistol" ? "Pistol" : -928);
    await game.input.trigger(weapon === "Pistol" ? "EquipPistol" : "EquipWrench");
    await game.step({ frames: 10 });
    const equipped = (await game.info()).player;
    assert.ok(equipped.wielded_entity_id != null);
    assert.equal((await game.entities.detail(equipped.wielded_entity_id)).name, weapon);
    assert.ok(equipped.active_psi_powers.includes("Inviso"), "equipping alone keeps stealth");
    await pullTrigger(game);
    assert.ok(!(await game.info()).player.active_psi_powers.includes("Inviso"), "attacking breaks stealth even on a miss");
  });
}
