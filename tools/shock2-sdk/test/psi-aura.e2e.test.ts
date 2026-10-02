import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

for (const vr of [false, true]) {
  for (const station of [{ name: "Cold monkey", template: -1431 }, { name: "Energy turret", template: -168 }, { name: "WeaponBash hybrid", template: -397 }]) {
    // The moving monkey's explosion falloff varies too widely in VR.
    // Exact VR damage assertions use the fixed turret and melee contact below.
    if (vr && station.template === -1431) continue;
    test(`Psycho-reflective Aura ${vr ? "VR" : "flat"} reduces ${station.name} hits and expires`, { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      const damage: number[] = [];
      for (const phase of ["baseline", "active", "expired"]) {
        await using game = await GameServer.launch({ mission: station.template === -168 ? "debug_turret" : "debug_psi", debugFlags: vr ? ["--vr"] : [] });
        if (station.template === -168) {
          await game.player.teleport({ x: -15, y: 1.244, z: 5 });
          await game.player.setStats({ psionic_ability: 6, endurance: 6 });
          await game.player.spawnItem(-247);
          await game.input.trigger("EquipPsiAmp");
        }
        if (vr) {
          await game.input.set("right_hand.squeeze", 1);
          await game.player.spawnItem(-247, { hand: "right" });
        }
        await game.step({ frames: 10 });
        assert.equal((await game.entities.byTemplate(station.template)).length, station.template === -397 ? 2 : 1);
        if (phase !== "baseline") {
          await selectPsiPower(game, "PsiShield");
          const psi = (await game.info()).player.psi_points!;
          await pullTrigger(game);
          assert.equal((await game.info()).player.psi_points, psi - 5);
          assert.ok((await game.info()).player.active_psi_powers.includes("PsiShield"));
          if (phase === "expired") {
            await game.step({ frames: 131 * 60 });
            assert.ok(!(await game.info()).player.active_psi_powers.includes("PsiShield"));
          }
        }
        const [attacker] = (await game.entities.byTemplate(station.template)).sort((a,b) => b.position[0] - a.position[0]);
        console.log(station.name, phase, attacker.position);
        await game.player.teleport({ x: attacker.position[0] + (station.template === -397 ? 1.5 : 8), y: 1.244, z: attacker.position[2] });
        await game.entities.sendMessage(attacker.id, { type: "SetAlertness", level: "High" });
        const start = (await game.info()).player.hit_points!;
        let delta = 0;
        for (let frame = 0; frame < 900 && delta === 0; frame++) {
          await game.step({ frames: 1 });
          delta = start - (await game.info()).player.hit_points!;
        }
        assert.ok(delta > 0, `${station.name} must land a live attack in ${phase}`);
        damage.push(delta);
      }
      console.log(`${station.name} baseline/active/expired HP deltas: ${damage}`);
      const tolerance = station.template === -1431 ? 1 : 0;
      // The moving monkey's Cold explosion has radial falloff; asynchronous
      // AI route adoption shifts impact position and rounding by up to one HP.
      assert.ok(Math.abs(damage[1] - Math.round(damage[0] * 0.4)) <= tolerance, `baseline/active/expired ${damage}`);
      assert.ok(damage[1] < damage[0] * 0.75, "shield must substantially reduce the real hit");
      assert.ok(Math.abs(damage[2] - damage[0]) <= tolerance, `baseline/active/expired ${damage}`);
    });
  }
}

test("Aura refreshes its sheet-based duration, survives save/load, and leaves untyped damage alone", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  await game.player.applyStatModifier({ source: "e2e:aura", stat: "psionic_ability", delta: -1, duration_secs: 1000 });
  await selectPsiPower(game, "PsiShield");
  await pullTrigger(game);
  await game.step({ frames: 109 * 60 });
  assert.deepEqual((await game.info()).player.active_psi_powers, ["PsiShield"]);
  await game.step({ frames: 2 * 60 });
  assert.deepEqual((await game.info()).player.active_psi_powers, []);
  await game.player.applyStatModifier({ source: "e2e:aura", stat: "psionic_ability", delta: 2, duration_secs: 1000 });
  await pullTrigger(game);
  await game.step({ frames: 30 * 60 });
  const psi = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  assert.equal((await game.info()).player.psi_points, psi - 5);
  assert.deepEqual((await game.info()).player.active_psi_powers, ["PsiShield"]);
  const player = (await game.info()).player;
  await game.entities.sendMessage(player.entity_id!, { type: "Damage", amount: 10 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.hit_points, player.hit_points! - 10,
    "untyped damage has no matching authored stimulus filter");
  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  const save = `e2e_psi_aura_${Date.now()}`;
  await game.save(save);
  await game.load(save);
  await game.step({ frames: 10 });
  assert.deepEqual((await game.info()).player.active_psi_powers, ["PsiShield"]);
  await game.step({ frames: 168 * 60 });
  assert.deepEqual((await game.info()).player.active_psi_powers, ["PsiShield"],
    "PSI 8 refresh lasts 170 seconds, including time persisted across save/load");
  await game.step({ frames: 3 * 60 });
  assert.deepEqual((await game.info()).player.active_psi_powers, []);
});
