import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

test("Energy Reflection preserves authored player Psi Mine immunity", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  const playerId = (await game.info()).player.entity_id!;
  async function blast(): Promise<number> {
    await selectPsiPower(game, "PsiMines");
    await pullTrigger(game);
    const [mine] = await game.entities.byTemplate(-3397);
    assert.ok(mine, "cast produces the authored mine");
    await game.player.teleport({ x: mine.position[0] + 1, y: mine.position[1], z: mine.position[2] });
    await game.step({ frames: 1 });
    const before = (await game.info()).player.hit_points!;
    // Detonate the real mine beside its caster. Damage addresses the mine,
    // not the player; its Corpse link emits the gamesys Psi Stim radius blast.
    await game.entities.sendMessage(mine.id, { type: "Damage", amount: 1 });
    await game.step({ frames: 10 });
    const damage = before - (await game.info()).player.hit_points!;
    await game.entities.sendMessage(playerId, { type: "Damage", amount: -damage });
    await game.step({ frames: 1 });
    return damage;
  }
  const baseline = await blast();
  assert.equal(baseline, 0, "authored Human Vulnerability has no Psi Stim damage response");
  await selectPsiPower(game, "AntiPsi");
  const psi = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  assert.equal((await game.info()).player.psi_points, psi - 3);
  assert.ok((await game.info()).player.active_psi_powers.includes("AntiPsi"));
  assert.equal(await blast(), 0, "active reflection preserves the player’s native Psi immunity");
  await game.step({ frames: 121 * 60 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("AntiPsi"));
  assert.equal(await blast(), baseline, "native Psi immunity remains after expiry");
});

for (const station of [{ name: "Cold monkey", template: -1431 }, { name: "Energy turret", template: -168 }, { name: "WeaponBash hybrid", template: -397 }]) {
  test(`Energy Reflection ${station.template === -397 ? "preserves" : "halves"} ${station.name} hits and expires`, { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
    const damage: number[] = [];
    for (const phase of ["baseline", "active", "expired"]) {
      await using game = await GameServer.launch({ mission: station.template === -168 ? "debug_turret" : "debug_psi" });
      if (station.template === -168) {
        await game.player.teleport({ x: -15, y: 1.244, z: 5 });
        await game.player.setStats({ psionic_ability: 6, endurance: 6 });
        await game.player.spawnItem(-247);
        await game.input.trigger("EquipPsiAmp");
      }
      await game.step({ frames: 10 });
      assert.equal((await game.entities.byTemplate(station.template)).length, station.template === -397 ? 2 : 1);
      if (phase !== "baseline") {
        await selectPsiPower(game, "AntiPsi");
        await pullTrigger(game);
        assert.ok((await game.info()).player.active_psi_powers.includes("AntiPsi"));
        if (phase === "expired") await game.step({ frames: 121 * 60 });
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
    assert.ok(Math.abs(damage[1] - Math.round(damage[0] * (station.template === -397 ? 1 : 0.5))) <= tolerance, `baseline/active/expired ${damage}`);
    if (station.template !== -397) assert.ok(damage[1] < damage[0] * 0.75, "shield must substantially reduce the real hit");
    assert.ok(Math.abs(damage[2] - damage[0]) <= tolerance, `baseline/active/expired ${damage}`);
  });
}
