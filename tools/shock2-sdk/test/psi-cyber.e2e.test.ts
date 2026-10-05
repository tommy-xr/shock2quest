import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { clickUiElement } from "./helpers/ui.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const vr of [false, true]) {
  test(`Psychogenic Cyber Affinity grants its authored temporary +2 CYB (${vr ? "VR" : "flat"})`, {
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
    await game.step({ frames: 1 });
    const before = (await game.info()).player;
    await selectPsiPower(game, "Cyber");
    await pullTrigger(game);
    const active = (await game.info()).player;
    assert.equal(active.psi_points, before.psi_points! - 1);
    assert.ok(active.active_psi_powers.includes("Cyber"));
    assert.equal(active.stats!.cyber_affinity, before.stats!.cyber_affinity, "the trained level stays unchanged");
    assert.equal(active.effective_stats!.cyber_affinity, before.effective_stats!.cyber_affinity + 2);
    const modifier = active.stats!.modifiers.find(m => m.source === "psi:cyber")!;
    assert.equal(modifier.remaining.secs, 239, "authored duration is 60 + 30 × effective PSI (6)");
    await game.step({ frames: 238 * 60 });
    assert.ok((await game.info()).player.active_psi_powers.includes("Cyber"));
    await game.step({ frames: 3 * 60 });
    const expired = (await game.info()).player;
    assert.equal(expired.effective_stats!.cyber_affinity, before.effective_stats!.cyber_affinity);
    assert.deepEqual(expired.active_psi_powers, []);
    assert.equal(expired.stats!.modifiers.filter(m => m.source === "psi:cyber").length, 0);
  });
}

test("Cyber changes real hack odds, refreshes without stacking and survives save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 30 });
  const [computer] = await game.entities.byTemplate(-1250);
  assert.ok(computer, "the hack station uses the authored security computer");
  await game.player.teleport({ x: -2, y: 1.3, z: 3.5 });
  async function boardText() {
    await game.entities.sendMessage(computer.id, { type: "Frob" });
    await game.step({ frames: 2 });
    let panel = (await game.ui.state()).active_panel;
    assert.equal(panel?.entity_id, computer.id);
    const hack = panel.elements.find(e => e.label === "hack-security");
    assert.ok(hack);
    await clickUiElement(game, hack);
    panel = (await game.ui.state()).active_panel!;
    const text = panel.elements.map(e => e.text ?? "").join("\n");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    return text;
  }
  const before = await boardText();
  await selectPsiPower(game, "Cyber");
  await pullTrigger(game);
  const after = await boardText();
  assert.notEqual(after, before, "the real hack board must consume effective CYB");
  assert.match(before, /CYB.*6/);
  assert.match(after, /CYB.*8/);
  await game.step({ frames: 60 });
  await pullTrigger(game);
  const refreshed = (await game.info()).player;
  assert.equal(refreshed.effective_stats!.cyber_affinity, 8);
  assert.equal(refreshed.stats!.modifiers.filter(m => m.source === "psi:cyber").length, 1);
  assert.equal(refreshed.stats!.modifiers.find(m => m.source === "psi:cyber")!.remaining.secs, 239);
  await game.transitionLevel("earth.mis");
  const saveName = `e2e_psi_cyber_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  const loaded = (await game.info()).player;
  assert.equal(loaded.stats!.cyber_affinity, 6);
  assert.equal(loaded.effective_stats!.cyber_affinity, 8);
  assert.deepEqual(loaded.active_psi_powers, ["Cyber"]);
  await game.step({ frames: 241 * 60 });
  assert.equal((await game.info()).player.effective_stats!.cyber_affinity, 6);
  assert.deepEqual((await game.info()).player.active_psi_powers, []);
});
