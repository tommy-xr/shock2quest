import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
async function property(game: GameServer, id: number, name: string) {
  return (await game.entities.detail(id)).properties.find(p => p.name === name)?.value;
}

for (const vr of [false, true]) {
  test(`PsiCharm allies attack and expire (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 300_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      if (vr) {
        const [amp] = await game.entities.byTemplate(-247);
        await aimVrHandAt(game, amp.position, 0.35);
        await game.input.set("right_hand.squeeze", 1);
        await game.step({ frames: 8 });
        assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
      }
      await selectPsiPower(game, "PsiCharm");
      await game.step({ frames: 15 });
      const targets = (await game.entities.list({ filter: "OG-Pipe", limit: 10 })).entities.sort((a,b) => b.position[0] - a.position[0]);
      const near = targets[0], far = targets[1];
      assert.ok(near && far);
      if (vr) {
        const torso = ((await game.entities.detail(near.id)).aim_points ?? []).find(p => p.classification === "torso")!;
        await aimVrHandAt(game, torso.position, 2.0, 1);
      } else {
        await game.input.set("head.look", [0, 3]);
      }
      const psi = (await game.info()).player.psi_points!;
      await pullTrigger(game);
      await game.step({ frames: 120 });
      assert.equal((await game.info()).player.psi_points, psi - 5);
      assert.equal(await property(game, near.id, "AITeam"), "Good");
      assert.equal(await property(game, near.id, "HitPoints"), "12", "charm does not damage its victim");
      await game.step({ frames: 900 });
      assert.ok(Number(await property(game, far.id, "HitPoints")) < 12, "the ally attacks the other hybrid");
      const ally = near;
      await game.step({ frames: 1500 });
      assert.equal(await property(game, ally.id, "AITeam"), "Good", "the 60-second timer is still pending");
      await game.step({ frames: 1800 });
      assert.equal(await property(game, ally.id, "AITeam"), "Bad1", "60 seconds at PSI6 restores the original team");
    });
}

test("PsiCharm saves its remaining timer in a real mission and player damage cancels it", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  const { readFile, writeFile, unlink } = await import("node:fs/promises");
  const { join } = await import("node:path");
  assert.ok(process.env.DARK_ASSET_PATH, "this fixture needs the explicitly selected asset root");
  const file = `e2e_charm_${Date.now()}`;
  const path = join(process.env.DARK_ASSET_PATH!, "saves", `${file}.sav`);
  await using game = await GameServer.launch({ mission: "earth.mis" });
  try {
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: 6, psi_tier: 5 });
    await game.player.spawnItem("Psi Amp");
    await game.input.trigger("EquipPsiAmp");
    await game.step({ frames: 5 });
    await game.save(file);
    // Provision learned disciplines and a full pool only; all charm state below
    // comes from the production bolt, and the tested save is written by the game.
    const fixture = JSON.parse(await readFile(path, "utf8"));
    fixture.global_data.quest_info.player_stats.purchased_psi_powers = [-3161, -3160];
    fixture.global_data.player_vitals.psi_points.current = fixture.global_data.player_vitals.psi_points.maximum;
    await writeFile(path, JSON.stringify(fixture));
    await game.load(file);
    await game.step({ frames: 5 });
    const before = new Set((await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(e => e.id));
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 15 });
    let target = (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.find(e => !before.has(e.id))!;
    assert.ok(target);
    await game.entities.sendMessage(target.id, { type: "Damage", amount: -100 });
    await selectPsiPower(game, "PsiCharm");
    await game.player.aimAt(target.id);
    await pullTrigger(game);
    await game.step({ frames: 180 });
    assert.equal(await property(game, target.id, "AITeam"), "Good");
    await game.save(file);
    await game.load(file);
    await game.step({ frames: 1 });
    const candidates = (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities;
    for (const candidate of candidates) {
      if (await property(game, candidate.id, "AITeam") === "Good") target = candidate;
    }
    assert.equal(await property(game, target.id, "AITeam"), "Good");
    await game.step({ frames: 2700 });
    assert.equal(await property(game, target.id, "AITeam"), "Good");
    await game.step({ frames: 900 });
    assert.equal(await property(game, target.id, "AITeam"), "Bad1");
    // Reload the already-proven charmed save for the independent cancellation
    // check; a second slow bolt at a now-wandering target would test aim timing.
    await game.load(file);
    await game.step({ frames: 1 });
    for (const candidate of (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities) {
      if (await property(game, candidate.id, "AITeam") === "Good") target = candidate;
    }
    assert.equal(await property(game, target.id, "AITeam"), "Good");
    // A player-authored Soma hit cancels charm. The target was given enough HP
    // to remain alive, so this is cancellation rather than corpse cleanup.
    const p = (await game.entities.detail(target.id)).position;
    await game.player.teleport({ x: p[0], y: p[1], z: p[2] + 2 });
    await game.step({ frames: 2 });
    await selectPsiPower(game, "SomaDrain");
    await game.player.aimAt(target.id);
    await pullTrigger(game);
    await game.step({ frames: 5 });
    assert.equal(await property(game, target.id, "AITeam"), "Bad1");
    assert.ok(Number(await property(game, target.id, "HitPoints")) > 0);
  } finally {
    await unlink(path).catch(() => {});
  }
});
