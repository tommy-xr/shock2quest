import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { ammoOf, cycleToWeapon, fireOnce, pullTrigger } from "./helpers/weapon.js";
import { equipRightHand } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`Stability prevents wear until its stat-scaled expiry (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 30 });
    const [amp] = await game.entities.byTemplate(-247);
    const pistol = await cycleToWeapon(game, e => e.template_id === -17);
    const equip = (id: number, action: "EquipPistol" | "EquipPsiAmp") =>
      equipRightHand(game, vr, id, action);
    async function shoot(count: number, expected: number) {
      const ammo = ammoOf(await game.entities.detail(pistol.id));
      for (let i = 0; i < count; i++) await fireOnce(game);
      assert.equal(ammoOf(await game.entities.detail(pistol.id)), ammo - count, "shots actually fired");
      assert.equal((await game.info()).player.wielded_gun_condition, expected);
    }
    await equip(pistol.id, "EquipPistol");
    assert.equal((await game.info()).player.wielded_gun_condition, 100);
    await shoot(2, 98);
    await equip(amp.id, "EquipPsiAmp");
    await selectPsiPower(game, "Stability");
    const before = (await game.info()).player.psi_points!;
    await pullTrigger(game);
    assert.equal((await game.info()).player.psi_points, before - 2);
    assert.ok((await game.info()).player.active_psi_powers.includes("Stability"));
    await equip(pistol.id, "EquipPistol");
    await shoot(3, 98);
    await game.step({ frames: 110 * 60 });
    assert.equal((await game.info()).player.life_state, "alive");
    assert.ok((await game.info()).player.active_psi_powers.includes("Stability"), "PSI 6 lasts 130 seconds");
    await shoot(1, 98);
    await game.step({ frames: 25 * 60 });
    assert.ok(!(await game.info()).player.active_psi_powers.includes("Stability"));
    await shoot(1, 97);
  });
}
