import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { acquireOsUpgrade, clickElement } from "./helpers/os-upgrade.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";

const enabled = process.env.SHOCK2_E2E === "1";
async function elements(game: GameServer) { return (await game.ui.state()).active_panel!.elements; }
async function click(game: GameServer, label: string) {
  const el = (await elements(game)).find(e => e.label === label);
  assert.ok(el, `button ${label}`);
  await clickElement(game, el);
}
async function openSettings(game: GameServer) {
  if ((await game.ui.state()).mode !== "use") { await game.input.trigger("ToggleUseMode"); await game.step({ frames: 2 }); }
  const button = (await game.ui.state()).readout.find(e => e.label === "gun_setting");
  assert.ok(button);
  await clickElement(game, button);
}
async function quotedCost(game: GameServer) {
  const button = (await elements(game)).find(e => e.label?.startsWith("MODIFY ("));
  assert.ok(button, JSON.stringify(await elements(game)));
  return { cost: Number(button.label!.match(/\d+/)![0]), button };
}
async function property(game: GameServer, gun: number, name: string) {
  return (await game.entities.detail(gun)).properties.find(p => p.name === name)!.value;
}
async function winBoard(game: GameServer) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const els = await elements(game);
    const start = els.find(e => e.label === "start-hack" || e.label === "reset-hack");
    assert.ok(start, "paid attempt remains available");
    await clickElement(game, start);
    // Play free nodes through the real board. Avoid known mines; failure burns
    // a normal node, requiring a fresh paid board if no triple remains.
    const candidates = (await elements(game)).filter(e => e.label?.startsWith("node-"));
    for (const node of candidates) {
      const current = await elements(game);
      if (current.some(e => /win[hm]\.pcx/.test(e.texture ?? ""))) return;
      if (current.some(e => /fail[hm]\.pcx/.test(e.texture ?? ""))) break;
      const overlay = current.find(e => e.kind === "image" && e.rect[0] === node.rect[0] && e.rect[1] === node.rect[1]);
      if (overlay) continue;
      await clickElement(game, node);
    }
    if ((await elements(game)).some(e => /win[hm]\.pcx/.test(e.texture ?? ""))) return;
  }
  assert.fail("20 real paid HRM attempts did not win");
}

test("Tinker halves actual paid Modify attempts; two pistol modifications survive save and transition", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.player.setStats({ skills: { modify: 6 }, cyber_affinity: 6 });
  await game.player.spawnItem(-17);
  await game.input.trigger("EquipPistol"); await game.step({ frames: 3 });
  let gun = (await game.info()).player.wielded_entity_id!;
  const base = JSON.parse(await property(game, gun, "GunDescription"));
  await openSettings(game);
  const before = await quotedCost(game);
  await clickElement(game, before.button);
  const balance = await carriedNaniteTotal(game);
  await click(game, "start-hack");
  assert.equal(await carriedNaniteTotal(game), balance, "insufficient payment leaves balance unchanged");
  assert.equal(await property(game, gun, "Modification"), "0");
  assert.ok((await elements(game)).some(e => /pay[hm]\.pcx/.test(e.texture ?? "")));
  await click(game, "Back to settings");
  await game.input.trigger("ToggleUseMode"); await game.step({ frames: 2 });
  await acquireOsUpgrade(game, "Tinker");
  await openSettings(game);
  const after = await quotedCost(game);
  assert.equal(after.cost, Math.max(1, Math.floor(before.cost / 2)));
  for (let i = 0; i < 20; i++) await game.player.spawnItem("20 Nanites");
  await clickElement(game, after.button);
  const paidBefore = await carriedNaniteTotal(game);
  await click(game, "start-hack");
  assert.equal(paidBefore - await carriedNaniteTotal(game), after.cost);
  // First paid board may be reset; each reset is another honest payment.
  await winBoard(game);
  assert.equal(await property(game, gun, "Modification"), "1");
  const first = JSON.parse(await property(game, gun, "GunDescription"));
  assert.equal(first.settings[0].clip, base.settings[0].clip + 12);
  assert.ok(Math.abs(first.settings[0].stim_modifier - base.settings[0].stim_modifier * 1.1) < 0.0001);
  await click(game, "Back to settings");
  await clickElement(game, (await quotedCost(game)).button);
  await winBoard(game);
  assert.equal(await property(game, gun, "Modification"), "2");
  const second = await property(game, gun, "GunDescription");
  assert.equal(JSON.parse(second).settings[0].reload_time_ms, Math.floor(base.settings[0].reload_time_ms / 3));
  await click(game, "Back to settings");
  assert.ok(!(await elements(game)).some(e => e.label?.startsWith("MODIFY (")), "no third modification quote");
  const save = `os_tinker_${Date.now()}`;
  await game.save(save); await game.load(save); await game.step({ frames: 2 });
  gun = (await game.entities.byTemplate(-17))[0].id;
  assert.equal(await property(game, gun, "Modification"), "2");
  assert.equal(await property(game, gun, "GunDescription"), second);
  await game.transitionLevel("earth.mis"); await game.step({ frames: 2 });
  gun = (await game.player.inventory()).items.find(i => i.name === "Pistol")!.entity_id;
  assert.equal(await property(game, gun, "Modification"), "2");
  assert.equal(await property(game, gun, "GunDescription"), second);
});
