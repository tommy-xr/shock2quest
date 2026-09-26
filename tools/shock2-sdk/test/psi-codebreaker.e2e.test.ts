import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

// Retail psihelp.str Psi7: reduces active alarms by 5 + 5 seconds per PSI.
test("Remote Electron Tampering shortens security alarms and stands security down", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_camera" });
  await game.step({ frames: 1 });
  await game.player.spawnItem(-247);
  await game.input.trigger("EquipPsiAmp");
  await game.step({ frames: 1 });
  await selectPsiPower(game, "Codebreaker");
  for (let i = 0; i < 20 && !(await game.ui.state()).security_alarm; i++) {
    await game.step({ frames: 60 });
  }
  const alarm = (await game.ui.state()).security_alarm;
  assert.ok(alarm, "camera should raise its real authored alarm");
  const before = (await game.info()).player;
  await pullTrigger(game);
  const after = (await game.info()).player;
  assert.equal(after.psi_points, before.psi_points! - 1, "successful cast costs one point");
  const remaining = (await game.ui.state()).security_alarm;
  assert.ok(remaining);
  const reduction = 5 + 5 * before.effective_stats!.psionic_ability;
  assert.ok(Math.abs(alarm.seconds_remaining - remaining.seconds_remaining - reduction) < 1,
    `expected ${reduction}s reduction, got ${alarm.seconds_remaining - remaining.seconds_remaining}`);
  // Repeated casts cross zero and reset the ecology, rather than just hiding its badge.
  for (let i = 0; i < 20 && (await game.ui.state()).security_alarm; i++) await pullTrigger(game);
  await game.step({ frames: 5 });
  assert.equal((await game.ui.state()).security_alarm ?? null, null);
  const [ecology] = await game.entities.byTemplate(-975);
  assert.equal((await game.entities.detail(ecology.id)).properties.find(p => p.name === "EcologyState")?.value, "Normal");
  const calmPoints = (await game.info()).player.psi_points;
  await pullTrigger(game);
  assert.equal((await game.info()).player.psi_points, calmPoints, "no alarm spends nothing");
});
