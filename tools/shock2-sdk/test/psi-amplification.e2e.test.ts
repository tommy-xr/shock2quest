import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

async function cryoTier(game: GameServer): Promise<string[]> {
  await selectPsiPower(game, "Cryokinesis");
  // Fire into clear space, keeping the projectile alive for inspection.
  await game.input.set("head.look", [180, 0]);
  const existing = new Set((await game.entities.list({ limit: 300 })).entities.map(e => e.id));
  await pullTrigger(game);
  return (await game.entities.list({ limit: 300 })).entities
    .filter(e => !existing.has(e.id) && e.name?.startsWith("Cryo PSI"))
    .map(e => e.name!);
}

for (const vr of [false, true]) {
  test(`Recursive Psionic Amplification raises effective PSI by its authored bonus (${vr ? "VR" : "flat"})`, {
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
    const before = (await game.info()).player;
    await selectPsiPower(game, "PsiImage");
    await pullTrigger(game);
    const active = (await game.info()).player;
    assert.equal(active.psi_points, before.psi_points! - 2);
    assert.ok(active.active_psi_powers.includes("PsiImage"));
    assert.equal(active.stats!.psionic_ability, 6, "training is unchanged");
    assert.equal(active.effective_stats!.psionic_ability, 8);
    const modifiers = active.stats!.modifiers.filter(m => m.source === "psi:image");
    assert.equal(modifiers.length, 1);
    assert.equal(modifiers[0].delta, 2);
    assert.equal(modifiers[0].remaining.secs, 69, "first cast uses pre-buff PSI 6: 70 seconds");
    if (!vr) {
      assert.deepEqual(await cryoTier(game), ["Cryo PSI 8"]);
      const remaining = (await game.info()).player.stats!.modifiers
        .find(m => m.source === "psi:image")!.remaining;
      await game.step({ frames: Math.floor((remaining.secs + remaining.nanos / 1e9) * 60) - 2 });
      let expired = false;
      for (let frame = 0; frame < 10; frame++) {
        await game.step({ frames: 1 });
        const player = (await game.info()).player;
        if (!player.active_psi_powers.includes("PsiImage")) {
          assert.equal(player.effective_stats!.psionic_ability, 6,
            "the bonus cannot outlive the active power at the expiry boundary");
          assert.ok(!player.stats!.modifiers.some(m => m.source === "psi:image"));
          expired = true;
          break;
        }
      }
      assert.ok(expired, "the initial cast expires after 70 seconds");
      assert.deepEqual(await cryoTier(game), ["Cryo PSI 6"]);
    }
  });
}

test("Amplification refreshes once, survives transition/save, then expires", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  assert.deepEqual(await cryoTier(game), ["Cryo PSI 6"]);
  await selectPsiPower(game, "PsiImage");
  await pullTrigger(game);
  await game.step({ frames: 30 * 60 });
  const spent = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  let player = (await game.info()).player;
  assert.equal(player.psi_points, spent - 2);
  assert.deepEqual(player.active_psi_powers, ["PsiImage"]);
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:image").length, 1);
  assert.equal(player.stats!.modifiers.find(m => m.source === "psi:image")!.remaining.secs, 89,
    "refresh uses current PSI 8: 90 seconds, without stacking the bonus");
  assert.equal(player.effective_stats!.psionic_ability, 8);
  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  const saveName = `e2e_psi_amplification_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  await game.step({ frames: 10 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.psionic_ability, 8);
  assert.deepEqual(player.active_psi_powers, ["PsiImage"]);
  // A refreshed cast at effective PSI 8 lasts 10 + 10*8 = 90 seconds.
  await game.step({ frames: 91 * 60 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.psionic_ability, 6);
  assert.deepEqual(player.active_psi_powers, []);
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:image").length, 0);
});

test("Amplification increases a real Soma Transference hit", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  async function hit(amplified: boolean): Promise<number> {
    await using game = await GameServer.launch({ mission: "debug_minimal" });
    await game.step({ frames: 30 });
    await game.player.setStats({ psionic_ability: 4 });
    await game.player.spawnItem(-247);
    await game.input.trigger("EquipPsiAmp");
    await game.step({ frames: 10 });
    if (amplified) {
      await selectPsiPower(game, "PsiImage");
      await pullTrigger(game);
    }
    await selectPsiPower(game, "SomaDrain");
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 10 });
    const [hybrid] = await game.entities.byTemplate(-397);
    assert.ok(hybrid, "the real cast needs a live hybrid target");
    const hp = async () => Number((await game.entities.detail(hybrid.id)).properties
      .find(p => p.name === "HitPoints")!.value);
    assert.equal(await hp(), 12);
    await pullTrigger(game);
    await game.step({ frames: 60 });
    return hp();
  }
  assert.equal(await hit(false), 2, "PSI 4 deals 10 damage, leaving the hybrid alive");
  assert.equal(await hit(true), 0, "PSI 6 deals a lethal 15 damage");
});
