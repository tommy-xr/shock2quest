import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade, standNear, clickElement } from "./helpers/os-upgrade.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";

test("Security Expert acquisition preserves retail security-board mines, cost and base Hack", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.step({ frames: 5 });
  await game.player.setStats({ skills: { hack: 1 } });
  await game.player.spawnItem("20 Nanites");
  await game.player.spawnItem("20 Nanites");
  const initialSkill = (await game.info()).player.stats!.skills.hack;
  const [computer] = await game.entities.byTemplate(209);
  assert.ok(computer);
  async function buyBoard() {
    await standNear(game, computer.id);
    await game.entities.sendMessage(computer.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).active_panel;
    assert.equal(panel?.entity_id, computer.id);
    const start = panel.elements.find(e => e.label === "start-hack" || e.label === "reset-hack");
    assert.ok(start);
    const balance = await carriedNaniteTotal(game);
    await clickElement(game, start);
    const cost = balance - await carriedNaniteTotal(game);
    const mines = (await game.ui.state()).active_panel!.elements.filter(e => e.texture?.toLowerCase().endsWith("hrmmine.pcx")).length;
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    return { cost, mines };
  }
  const before = await buyBoard();
  assert.ok(before.mines > 0, "ordinary security hacking starts with a mine");
  await acquireOsUpgrade(game, "Security Expert");
  const after = await buyBoard();
  // Retail HRM skill_critical_bonus is zero: the +2 effective skill improves
  // success chance (covered by the resolver test), not the authored mines.
  assert.equal(after.mines, before.mines);
  assert.equal(after.cost, before.cost);
  assert.ok(after.cost > 0);
  assert.equal((await game.info()).player.stats!.skills.hack, initialSkill);
});
