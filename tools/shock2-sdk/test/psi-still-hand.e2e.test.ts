import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { ammoOf, cycleToWeapon, fireOnce, pullTrigger, muzzleFrameOf } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// Original shkplgun.cpp::CalcKickAngle returns zero for Still Hand.
// Backward travel remains authored; this verifies angular recoil without moving the head.
for (const vr of [false, true]) {
  test(`Still Hand removes angular recoil until expiry (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr", "--experimental", "physical_held_items"] : [] });
    await game.step({ frames: 30 });
    await game.devParams.set("gun_agility_override", 1);
    const [amp] = await game.entities.byTemplate(-247);
    const pistol = await cycleToWeapon(game, e => e.template_id === -17);
    async function equip(id: number, action: "EquipPistol" | "EquipPsiAmp") {
      if (vr) {
        await game.input.set("right_hand.squeeze", 0);
        await game.step({ frames: 3 });
        await aimVrHandAt(game, (await game.entities.detail(id)).position, 0.3);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, id);
      } else {
        await game.input.trigger(action);
        await game.step({ frames: 10 });
      }
    }
    async function angularKick(): Promise<number> {
      await game.step({ frames: 180 });
      const rest = muzzleFrameOf(await game.entities.detail(pistol.id)).forward;
      const head = (await game.info()).player.camera_rotation;
      const ammo = ammoOf(await game.entities.detail(pistol.id));
      await fireOnce(game);
      let peak = 0;
      for (let i = 0; i < 12; i++) {
        await game.step({ frames: 1 });
        const forward = muzzleFrameOf(await game.entities.detail(pistol.id)).forward;
        const dot = rest.reduce((sum, v, j) => sum + v * forward[j]!, 0);
        peak = Math.max(peak, Math.acos(Math.max(-1, Math.min(1, dot))) * 180 / Math.PI);
      }
      assert.equal(ammoOf(await game.entities.detail(pistol.id)), ammo - 1);
      assert.deepEqual((await game.info()).player.camera_rotation, head, "the tracked head never recoils");
      return peak;
    }
    await equip(pistol.id, "EquipPistol");
    const baseline = await angularKick();
    assert.ok(baseline > 0.2, `baseline must have observable kick: ${baseline}`);
    await equip(amp.id, "EquipPsiAmp");
    await selectPsiPower(game, "Still Hand");
    const before = (await game.info()).player.psi_points!;
    await pullTrigger(game);
    assert.equal((await game.info()).player.psi_points, before - 1);
    assert.ok((await game.info()).player.active_psi_powers.includes("Still Hand"));
    await equip(pistol.id, "EquipPistol");
    const protectedKick = await angularKick();
    assert.ok(protectedKick < 0.1, `Still Hand eliminates angular kick: ${protectedKick}; baseline ${baseline}`);
    await game.step({ frames: 165 * 60 });
    assert.equal((await game.info()).player.life_state, "alive");
    assert.ok((await game.info()).player.active_psi_powers.includes("Still Hand"), "PSI 6 lasts 180 seconds");
    await game.step({ frames: 20 * 60 });
    assert.ok(!(await game.info()).player.active_psi_powers.includes("Still Hand"));
    const expiredKick = await angularKick();
    assert.ok(expiredKick > 0.2, `kick returns after expiry: ${expiredKick}`);
  });
}
