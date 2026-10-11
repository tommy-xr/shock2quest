import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync, writeFileSync } from "node:fs";
import { savedGamePath } from "./helpers/save.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { GameServer, type UiElement } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`armor: ${vr ? "VR" : "flat"} inventory equips and chest removes powered armor`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", port: 0, debugFlags: vr ? ["--vr"] : [] });
    await game.player.setStats({ strength: 6 });
    const id = (await game.player.spawnItem(-82)).entity_id;
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const item = (await game.ui.state()).strip!.elements.find(e => e.entity_id === id && e.kind === "button")!;
    assert.ok(item);
    const click = async (element: UiElement) => {
      if (vr) await clickCanvasWithRay(game, requirePanelPose(await game.ui.state()), canvasCenter(element));
      else await clickUiElement(game, element);
    };
    await click(item);
    if (!vr) await click(item);
    let armor = (await game.ui.state()).strip!.elements.find(e => e.label === "Armor");
    assert.equal(armor?.entity_id, id, "inventory USE equips power armor in the chest slot");
    await click(armor!);
    armor = (await game.ui.state()).strip!.elements.find(e => e.label === "Armor");
    assert.equal(armor?.entity_id, null, "chest click removes armor");
    assert.ok((await game.player.inventory()).items.some(i => i.entity_id === id));
  });
}

async function use(game: GameServer, entity: number) {
  await game.entities.sendMessage(entity, { type: "Frob" });
  await game.step({ frames: 2 });
}
async function property(game: GameServer, id: number, name: string) {
  return (await game.entities.detail(id)).properties.find(p => p.name === name)?.value;
}

test("armor: requirements, one slot, hazards, and Worm Skin bonus/cost survive saves", {
  skip: !enabled, timeout: 360_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0, debugFlags: ["--vr"] });
  const heavy = (await game.player.spawnItem(-81)).entity_id;
  await use(game, heavy);
  assert.equal(await property(game, heavy, "ArmorEquipped"), "false");
  await game.player.setStats({ strength: 6 });
  await use(game, heavy);
  assert.equal(await property(game, heavy, "ArmorEquipped"), "true");
  const suit = (await game.player.spawnItem(-83)).entity_id;
  await use(game, suit);
  assert.equal(await property(game, heavy, "ArmorEquipped"), "false");
  const player = (await game.info()).player.entity_id!;
  await game.entities.sendMessage(player, { type: "Hazard", toxin: true, amount: 4 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.toxin_level, 1);
  // Cure the test exposure so it cannot contaminate the Worm Skin HP assertion.
  const detox = (await game.player.spawnItem("Detox Patch")).entity_id;
  await use(game, detox);
  let worm = (await game.player.spawnItem(-84)).entity_id;
  await use(game, worm);
  assert.equal(await property(game, worm, "ArmorEquipped"), "false", "research gates use");
  await game.entities.sendMessage(worm, { type: "SetObjectState", state: "Normal" });
  const before = (await game.info()).player;
  await use(game, worm);
  assert.equal((await game.info()).player.effective_stats!.psionic_ability, before.effective_stats!.psionic_ability + 2);
  assert.equal(await property(game, suit, "ArmorEquipped"), "false");
  const start = (await game.info()).player;
  await game.step({ frames: 29 * 60 });
  assert.equal((await game.info()).player.psi_points, start.psi_points);
  const save = `e2e_armor_${Date.now()}`;
  await game.save(save);
  // Seed the last PSI point in the real save; exercise depletion without
  // simulating the rest of a full pool on every suite run.
  const path = savedGamePath(save);
  const fixture = JSON.parse(readFileSync(path, "utf8"));
  fixture.global_data.player_vitals.psi_points.current = 1;
  writeFileSync(path, JSON.stringify(fixture));
  await game.load(save);
  worm = (await game.entities.byTemplate(-84))[0].id;
  await game.step({ frames: 60 });
  const drained = (await game.info()).player;
  assert.equal(drained.psi_points, 0, "saved timer spends the final PSI point after one more second");
  assert.equal(drained.hit_points, start.hit_points);
  assert.equal(await property(game, worm, "ArmorEquipped"), "true");
  const exhausted = (await game.info()).player;
  await game.step({ frames: 30 * 60 });
  assert.equal((await game.info()).player.hit_points, exhausted.hit_points! - 1, "Worm Skin costs health once PSI runs out");
  await use(game, worm);
  assert.equal((await game.info()).player.effective_stats!.psionic_ability, before.effective_stats!.psionic_ability);
  const removed = (await game.info()).player;
  await game.step({ frames: 31 * 60 });
  assert.equal((await game.info()).player.psi_points, removed.psi_points);
  assert.equal((await game.info()).player.hit_points, removed.hit_points);
});

test("armor: powered armor drains only while worn, depletes, and recharges at a station", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port: 0, debugFlags: ["--vr"] });
  const stats = await game.player.setStats({ strength: 6 });
  const capacity = 100 + 10 * stats.skills.maintenance;
  let armor = (await game.player.spawnItem(-82)).entity_id;
  const energy = async () => Number(await property(game, armor, "Energy"));
  await game.step({ frames: 600 });
  assert.equal(await energy(), 100);
  await use(game, armor);
  await game.step({ frames: 300 });
  assert.equal(await energy(), 99, "authored one charge per five seconds");
  const save = `e2e_armor_power_${Date.now()}`;
  await game.save(save);
  const path = savedGamePath(save);
  const fixture = JSON.parse(readFileSync(path, "utf8"));
  const held = fixture.global_data.held_items.held_entities;
  const equipped = held.hazard_equipment[0];
  held.properties["P$Energy"][String(equipped)] = 1;
  writeFileSync(path, JSON.stringify(fixture));
  await game.load(save);
  armor = (await game.entities.byTemplate(-82))[0].id;
  await game.step({ frames: 300 });
  assert.equal(await energy(), 0);
  assert.equal(await property(game, armor, "ArmorEquipped"), "true", "depletion leaves the suit equipped");
  assert.equal((await game.audio.recent({ sample: "bb07" })).sounds.length, 1);
  await game.transitionLevel("earth.mis");
  armor = (await game.entities.byTemplate(-82))[0].id;
  const station = (await game.entities.byTemplate(258))[0];
  assert.ok(station);
  await use(game, station.id);
  assert.equal(await energy(), capacity);
  await use(game, armor);
  await game.step({ frames: 600 });
  assert.equal(await energy(), capacity);
});

for (const [template, factor] of [[-79, .8], [-80, .7], [-81, .6], [-82, .5], [-83, 1], [-84, .8]] as const) {
  test(`armor: ${template} reduces a real hybrid melee hit by its authored amount`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", port: 0, debugFlags: ["--vr"] });
    await game.player.setStats({ strength: 6, endurance: 6 });
    const armor = (await game.player.spawnItem(template)).entity_id;
    if (template === -84) await game.entities.sendMessage(armor, { type: "SetObjectState", state: "Normal" });
    await use(game, armor);
    const attacker = (await game.entities.byTemplate(-397)).sort((a,b) => b.position[0] - a.position[0])[0];
    await game.player.teleport({ x: attacker.position[0] + 1.5, y: 1.244, z: attacker.position[2] });
    await game.entities.sendMessage(attacker.id, { type: "SetAlertness", level: "High" });
    const hp = (await game.info()).player.hit_points!;
    let damage = 0;
    for (let i = 0; i < 900 && damage === 0; i++) {
      await game.step({ frames: 1 });
      damage = hp - (await game.info()).player.hit_points!;
    }
    assert.equal(damage, Math.round(10 * factor));
  });
}

for (const hand of ["left", "right"] as const) {
  test(`armor: held ${hand} trigger equips and releasing into the world removes protection`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", port: 0, debugFlags: ["--vr"] });
    await game.player.setStats({ strength: 6 });
    await game.input.set(`${hand}_hand.position`, [hand === "left" ? -.5 : .5, 1, -1]);
    await game.input.set(`${hand}_hand.squeeze`, 1);
    const armor = (await game.player.spawnItem(-79, { hand })).entity_id;
    await game.step({ frames: 3 });
    await game.input.set(`${hand}_hand.trigger`, 1);
    await game.step({ frames: 3 });
    assert.equal(await property(game, armor, "ArmorEquipped"), "true");
    await game.input.set(`${hand}_hand.trigger`, 0);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 3 });
    assert.equal(await property(game, armor, "ArmorEquipped"), "false");
  });
}

for (const station of [{ mission: "debug_turret", template: -168, factor: .25 }, { mission: "debug_psi", template: -1431, factor: .5 }]) {
  test(`armor: power armor filters real ${station.template === -168 ? "energy contact" : "cold radius"} damage`, {
    skip: !enabled, timeout: 240_000,
  }, async () => {
    const damages: number[] = [];
    for (const wear of [false, true]) {
      await using game = await GameServer.launch({ mission: station.mission, port: 0 });
      await game.player.setStats({ strength: 6, endurance: 6 });
      const armor = (await game.player.spawnItem(-82)).entity_id;
      if (wear) await use(game, armor);
      const attacker = (await game.entities.byTemplate(station.template))[0];
      await game.player.teleport({ x: attacker.position[0] + 8, y: 1.244, z: attacker.position[2] });
      await game.entities.sendMessage(attacker.id, { type: "SetAlertness", level: "High" });
      const hp = (await game.info()).player.hit_points!;
      let damage = 0;
      for (let i = 0; i < 900 && damage === 0; i++) {
        await game.step({ frames: 1 });
        damage = hp - (await game.info()).player.hit_points!;
      }
      assert.ok(damage > 0);
      damages.push(damage);
    }
    assert.ok(Math.abs(damages[1] - Math.round(damages[0] * station.factor)) <= (station.template === -1431 ? 1 : 0), `baseline/protected damage: ${damages}`);
  });
}

for (const template of [-79, -82, -84]) {
  test(`armor: VR stowing equipped ${template} preserves ownership, protection, and timer phase`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", port: 0, debugFlags: ["--vr"] });
    await game.player.setStats({ strength: 6 });
    await game.input.set("right_hand.position", [.5, 1, -1]);
    await game.input.set("right_hand.squeeze", 1);
    const armor = (await game.player.spawnItem(template, { hand: "right" })).entity_id;
    if (template === -84) await game.entities.sendMessage(armor, { type: "SetObjectState", state: "Normal" });
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    const psi = (await game.info()).player.psi_points;
    if (template !== -79) await game.step({ frames: (template === -82 ? 4 : 29) * 60 });
    if ((await game.ui.state()).mode !== "use") await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
    const pose = requirePanelPose(await game.ui.state());
    await aimVrHandAtCanvas(game, pose, [320, 60], { squeeze: 1 });
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 3 });
    assert.ok((await game.player.inventory()).items.some(i => i.entity_id === armor && i.location === "inventory"));
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(await property(game, armor, "ArmorEquipped"), "true", "stowing is not unequipping");
    await game.step({ frames: 60 });
    if (template === -82) assert.equal(await property(game, armor, "Energy"), "99", "stowing preserves drain phase");
    if (template === -84) assert.equal((await game.info()).player.psi_points, psi! - 1, "stowing preserves PsiDrain phase");
  });
}
