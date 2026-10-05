import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { canvasCenter } from "./helpers/ui.js";
import { aimVrHandAtCanvas, drawPersonalCard, type Hand } from "./helpers/vr-hand.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

async function click(game: GameServer, element: UiElement, hand: Hand) {
  const pose = (await game.ui.state()).panel_pose;
  assert.ok(pose);
  for (const trigger of [0, 1, 0]) {
    await aimVrHandAtCanvas(game, pose, canvasCenter(element), { hand, trigger });
    await game.step({ frames: 3 });
  }
}

async function handItems(game: GameServer) {
  const player = (await game.info()).player;
  return { left: player.wielded_entity_id, right: player.right_hand_entity_id };
}

for (const hand of ["left", "right"] as const) {
  test(`empty ${hand} hand uses an inventory gun without equipping; squeeze still grabs`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", port: 0, debugFlags: ["--vr"] });
    const other = hand === "left" ? "right" : "left";
    await game.input.set(`${other}_hand.position`, [other === "left" ? -.4 : .4, .8, -1.2]);
    await game.input.set(`${other}_hand.rotation`, [0, 0, 0, 1]);
    await game.input.set(`${other}_hand.squeeze`, 1);
    const held = (await game.player.spawnItem(-19, { hand: other })).entity_id;
    const pistol = (await game.player.spawnItem(-17)).entity_id;
    await game.step({ frames: 5 });
    const before = await handItems(game);
    assert.equal(before[hand], null);
    assert.equal(before[other], held);
    const ammo = ammoOf(await game.entities.detail(held));
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const slot = (await game.ui.state()).strip?.elements.find(e => e.entity_id === pistol && e.kind === "button");
    assert.ok(slot);
    await click(game, slot, hand);
    await game.step({ frames: 20 });
    let panel = (await game.ui.state()).active_panel;
    assert.ok(panel);
    assert.ok(panel?.elements.some(e => e.text === "Pistol"));
    assert.deepEqual(await handItems(game), before, "USE preserves both hands");
    assert.ok((await game.player.inventory()).items.some(i => i.entity_id === pistol && i.location === "inventory"));
    const modify = panel.elements.find(e => e.label === "modify");
    assert.ok(modify);
    await click(game, modify, hand);
    assert.ok((await game.ui.state()).active_panel?.elements.some(e => e.label?.startsWith("upgrade_")));
    await game.entities.sendMessage(pistol, { type: "SetObjectState", state: "Broken" });
    await game.step({ frames: 2 });
    await click(game, slot, hand);
    panel = (await game.ui.state()).active_panel;
    const repair = panel?.elements.find(e => e.label === "repair");
    assert.ok(repair);
    await click(game, repair, hand);
    assert.ok((await game.ui.state()).active_panel?.elements.some(e => e.texture?.toLowerCase().includes("repair.pcx")));
    assert.deepEqual(await handItems(game), before, "maintenance does not equip the target");
    assert.equal(ammoOf(await game.entities.detail(held)), ammo);
    // The same slot still supports its distinct pickup gesture.
    const pose = (await game.ui.state()).panel_pose;
    assert.ok(pose);
    for (const squeeze of [0, 1]) {
      await aimVrHandAtCanvas(game, pose, canvasCenter(slot), { hand, squeeze });
      await game.step({ frames: 4 });
    }
    assert.deepEqual(await handItems(game), { ...before, [hand]: pistol });
  });
}

test("a gun fires while the other hand holds the tricorder", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons", port: 0, debugFlags: ["--vr"] });
  await game.input.set("right_hand.position", [.4, .8, -1.2]);
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.input.set("right_hand.squeeze", 1);
  const gun = (await game.player.spawnItem(-17, { hand: "right" })).entity_id;
  await game.step({ frames: 5 });
  const ammo = ammoOf(await game.entities.detail(gun));
  assert.ok(ammo > 0);
  await drawPersonalCard(game, "left");
  await game.input.set("left_hand.position", [-.3, 1.3, -.6]);
  await game.step({ frames: 10 });
  assert.equal((await game.ui.state()).mode, "device");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 5 });
  assert.ok(ammoOf(await game.entities.detail(gun)) < ammo);
  assert.ok(!(await game.ui.state()).active_panel?.elements.some(e => e.text === "Pistol"));
});
