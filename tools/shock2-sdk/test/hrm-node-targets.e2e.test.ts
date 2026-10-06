import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type UiElement } from "../src/index.js";
import { aimMfdAt, drawPersonalCard, add, sub, quatRotate, quatFromTo } from "./helpers/vr-hand.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement } from "./helpers/ui.js";
import { nodeOverlay } from "./helpers/hrm.js";

for (const vr of [false, true]) {
  test(`HRM accepts a click just outside node artwork (${vr ? "VR oblique ray" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.devParams.set("vr_mfd_focus_scan", 0);
    await game.devParams.set("hrm_force_critical", 1);
    await game.step({ frames: 10 });
    const [nanites] = await game.entities.byTemplate(257);
    await game.entities.sendMessage(nanites.id, { type: "Frob" });
    const [reader] = await game.entities.byTemplate(262);
    const [x, , z] = reader.position;
    await game.player.teleport({ x: x - 1.59, y: 21.404, z: z - 2.23 });
    await game.step({ frames: 120 });
    if (vr) {
      const aim = await game.player.aimAt(reader.id, { hitbox: "center", visibility: "required" });
      await drawPersonalCard(game, "left");
      await aimMfdAt(game, aim.world_point, .55, 1, 0, { hand: "left" });
      await game.input.set("left_hand.trigger", 1);
      await game.step({ frames: 12 });
      await game.input.set("left_hand.trigger", 0);
      await game.input.set("left_hand.position", [-.2, .3, -.6]);
      await game.input.set("left_hand.rotation", [0, 0, 0, 1]);
    } else await game.entities.sendMessage(reader.id, { type: "Frob" });
    await game.step({ frames: 4 });
    for (const label of ["hack-replicator", "start-hack"]) {
      const ui = await game.ui.state();
      const button = ui.active_panel?.elements.find(e => e.label === label);
      assert.ok(button, label);
      if (vr) await clickCanvasWithRay(game, ui.panel_pose!, canvasCenter(button));
      else await clickUiElement(game, button);
    }
    const ui = await game.ui.state();
    const node = ui.active_panel!.elements.find(e => e.label === "node-2-0")!;
    const art = nodeOverlay(ui.active_panel!.elements, node)!;
    assert.ok(art, "forced critical board makes every node visible as a mine");
    // Four authored pixels to the left of the artwork: inside the enlarged
    // target, outside the former 16px button. One click deterministically loses.
    const point: [number, number] = [art.rect[0] - art.rect[2] / 4, canvasCenter(art)[1]];
    if (vr) {
      const panel = ui.panel_pose!;
      const target = add(panel.center, quatRotate(panel.rotation, [
        (point[0] / panel.canvas[0] - .5) * panel.size[0],
        (.5 - point[1] / panel.canvas[1]) * panel.size[1], 0,
      ]));
      const origin = add(target, quatRotate(panel.rotation, [.6, 0, .5]));
      await game.input.set("right_hand.position", origin);
      await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], sub(target, origin)));
      for (const pressed of [0, 1, 0]) {
        await game.input.set("right_hand.trigger", pressed);
        await game.step({ frames: 3 });
      }
    } else {
      const near: UiElement = { ...art, screen_rect: [
        art.screen_rect[0] - art.screen_rect[2] / 4,
        art.screen_rect[1] + art.screen_rect[3] / 2, 0, 0,
      ] };
      await clickUiElement(game, near);
    }
    assert.equal((await game.entities.detail(reader.id)).properties.find(p => p.name === "ObjectState")?.value,
      "Broken", "the near-edge click must reach the node and resolve its forced critical outcome");
  });
}
