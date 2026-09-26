import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`Psychogenic Strength grants its authored temporary +2 STR (${vr ? "VR" : "flat"})`, {
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
    await selectPsiPower(game, "Might");
    await pullTrigger(game);
    const active = (await game.info()).player;
    assert.equal(active.psi_points, before.psi_points! - 2);
    assert.ok(active.active_psi_powers.includes("Might"));
    assert.equal(active.stats!.strength, before.stats!.strength, "the trained level stays unchanged");
    assert.equal(active.effective_stats!.strength, before.effective_stats!.strength + 2);
  });
}

test("Might refreshes once, survives level transition and save/load, then expires", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  await selectPsiPower(game, "Might");
  await pullTrigger(game);
  await game.step({ frames: 60 * 60 });
  const spent = (await game.info()).player.psi_points!;
  await pullTrigger(game);
  let player = (await game.info()).player;
  assert.equal(player.psi_points, spent - 2);
  assert.deepEqual(player.active_psi_powers, ["Might"]);
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:might").length, 1);
  assert.equal(player.effective_stats!.strength, 8);

  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  assert.equal((await game.info()).player.effective_stats!.strength, 8);
  const saveName = `e2e_psi_might_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  await game.step({ frames: 10 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.strength, 8);
  assert.deepEqual(player.active_psi_powers, ["Might"]);
  await game.step({ frames: 480 * 60 });
  player = (await game.info()).player;
  assert.equal(player.effective_stats!.strength, player.stats!.strength);
  assert.deepEqual(player.active_psi_powers, []);
  assert.equal(player.stats!.modifiers.filter(m => m.source === "psi:might").length, 0);
});

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(p => p.name === "HitPoints");
  assert.ok(property);
  return Number(property.value);
}

async function wrenchHit(game: GameServer): Promise<number> {
  let hybrid: EntityDetailResult | undefined;
  for (const entity of (await game.entities.list({ limit: 300 })).entities) {
    const detail = await game.entities.detail(entity.id);
    if ((detail.aim_points ?? []).length > 1) { hybrid = detail; break; }
  }
  assert.ok(hybrid, "expected a live creature in debug_melee");
  const [x, y, z] = hybrid.position;
  await game.player.teleport({ x: x + 1.1, y: y + 1, z });
  await game.step({ frames: 30 });
  const aim = await game.player.aimAt(hybrid.entity_id, { hitbox: "torso", visibility: "required" });
  assert.equal(aim.entity_id, hybrid.entity_id);
  const before = hitPoints(await game.entities.detail(hybrid.entity_id));
  await pullTrigger(game);
  await game.step({ frames: 60 });
  const after = hitPoints(await game.entities.detail(hybrid.entity_id));
  return before - after;
}

test("Might increases the damage of a real wrench hit", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  async function hit(might: boolean): Promise<number> {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 20 });
    await game.player.spawnItem(-928);
    if (might) {
      await game.player.spawnItem(-247);
      await game.input.trigger("EquipPsiAmp");
      await game.step({ frames: 10 });
      await selectPsiPower(game, "Might");
      await pullTrigger(game);
      assert.ok((await game.info()).player.active_psi_powers.includes("Might"));
    }
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 5 });
    return await wrenchHit(game);
  }
  const baseline = await hit(false);
  const buffed = await hit(true);
  assert.ok(baseline > 0 && buffed > baseline, `Might should improve a real hit: ${baseline} -> ${buffed}`);
});
