import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { acquireOsUpgrade, clickElement } from "./helpers/os-upgrade.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { closeSettings, elements, openSettings, property, reopenSettings, winBoard } from "./helpers/hrm.js";

const enabled = process.env.SHOCK2_E2E === "1";
async function click(game: GameServer, label: string) {
  const el = (await elements(game)).find(e => e.label === label);
  assert.ok(el, `button ${label}`);
  await clickElement(game, el);
}
/** Press the modify plug and read the quoted cost off the board it opens. */
async function openModifyBoard(game: GameServer) {
  await click(game, "modify");
  const cost = (await elements(game)).find(e => e.kind === "text" && /^\d+$/.test(e.text ?? ""));
  assert.ok(cost, JSON.stringify(await elements(game)));
  return Number(cost.text);
}
test("Tinker halves actual paid Modify attempts; two pistol modifications survive save and transition", { skip: !enabled, timeout: 300_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.player.setStats({ skills: { modify: 6 }, cyber_affinity: 6 });
  await game.player.spawnItem(-17);
  await game.input.trigger("EquipPistol"); await game.step({ frames: 3 });
  let gun = (await game.info()).player.wielded_entity_id!;
  const base = JSON.parse(await property(game, gun, "GunDescription"));
  await openSettings(game);
  const before = await openModifyBoard(game);
  const balance = await carriedNaniteTotal(game);
  await click(game, "start-hack");
  assert.equal(await carriedNaniteTotal(game), balance, "insufficient payment leaves balance unchanged");
  assert.equal(await property(game, gun, "Modification"), "0");
  assert.ok((await elements(game)).some(e => /pay[hm]\.pcx/.test(e.texture ?? "")));
  await closeSettings(game);
  await acquireOsUpgrade(game, "Tinker");
  for (let i = 0; i < 20; i++) await game.player.spawnItem("20 Nanites");
  await openSettings(game);
  const after = await openModifyBoard(game);
  assert.equal(after, Math.max(1, Math.floor(before / 2)));
  const paidBefore = await carriedNaniteTotal(game);
  await click(game, "start-hack");
  assert.equal(paidBefore - await carriedNaniteTotal(game), after);
  // First paid board may be reset; each reset is another honest payment.
  await winBoard(game);
  assert.equal(await property(game, gun, "Modification"), "1");
  const first = JSON.parse(await property(game, gun, "GunDescription"));
  assert.equal(first.settings[0].clip, base.settings[0].clip + 12);
  assert.ok(Math.abs(first.settings[0].stim_modifier - base.settings[0].stim_modifier * 1.1) < 0.0001);
  await reopenSettings(game);
  await openModifyBoard(game);
  await winBoard(game);
  assert.equal(await property(game, gun, "Modification"), "2");
  const second = await property(game, gun, "GunDescription");
  assert.equal(JSON.parse(second).settings[0].reload_time_ms, Math.floor(base.settings[0].reload_time_ms / 3));
  await reopenSettings(game);
  await click(game, "modify");
  assert.ok(!(await elements(game)).some(e => e.label === "start-hack"), "no third modification board");
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
