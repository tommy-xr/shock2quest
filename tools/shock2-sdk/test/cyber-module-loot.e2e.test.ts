import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";
import {
  GUI_PIXEL_TO_WORLD_SIZE,
  LOOT_PANEL_SIZE_PX,
  add,
  aimVrHandAt,
  quatRotate,
} from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

// MedSci1 corpse 490 authors cookie 1398 with StackCount 4. Exercise the
// shared visible slot and each native collection gesture, without granting
// modules or injecting Frob messages into either entity.
for (const mode of ["flat", "vr-trigger", "vr-grip"] as const) {
  test(`MedSci1 cyber-module corpse loot: ${mode}`, {
    skip: !enabled,
    timeout: 600_000,
  }, async () => {
    const vr = mode !== "flat";
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 0),
      debugFlags: vr ? ["--vr"] : [],
    });
    await game.step({ frames: 5 });
    const [corpse] = await game.entities.byTemplate(490);
    const [cookie] = await game.entities.byTemplate(1398);
    assert.ok(corpse && cookie, "the authored corpse and module pile must exist");
    const detail = await game.entities.detail(cookie.id);
    assert.equal(
      Number(detail.properties.find((p) => p.name === "StackCount")?.value),
      4,
    );
    assert.ok((await game.entities.detail(corpse.id)).outgoing_links.some(
      (link) => link.link_type.startsWith("Contains") && link.target_id === cookie.id,
    ));
    const before = (await game.info()).player.stats?.cyber_modules;
    assert.equal(typeof before, "number");

    // Isolate the real corpse interaction at a supported nearby approach.
    await teleportVerified(game, { x: -23, y: -6.755, z: -56.5 });
    await game.step({ frames: 3 });
    const aim = await game.player.aimAt(corpse.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    if (vr) await aimVrHandAt(game, aim.world_point, 0.35);
    const openInput = vr ? "right_hand.trigger" : "right_hand.squeeze";
    await game.input.set(openInput, 1);
    await game.step({ frames: 2 });
    await game.input.set(openInput, 0);
    await game.step({ frames: 4 });
    const panel = (await game.ui.state()).active_panel;
    assert.equal(
      panel?.entity_id,
      corpse.id,
      "native use must open the corpse loot panel",
    );
    await game.screenshot(`cyber-module-loot-${mode}-before.png`);
    const slot = panel.elements.find(
      (element) => element.kind === "button" && element.entity_id === cookie.id,
    );
    assert.ok(slot, "the real four-module pile must have a visible collectible slot");
    assert.ok(
      slot.texture?.toLowerCase().includes("upgrade"),
      "the slot must use the authored module icon",
    );

    if (vr) {
      const proxies = (await game.physics.bodies()).bodies.filter(
        (body) => body.collision_groups.includes("ui"),
      );
      assert.equal(proxies.length, 1);
      const [proxy] = proxies;
      const u = (slot.rect[0] + slot.rect[2] / 2) / LOOT_PANEL_SIZE_PX[0];
      const v = (slot.rect[1] + slot.rect[3] / 2) / LOOT_PANEL_SIZE_PX[1];
      const target = add(proxy.position, quatRotate(proxy.rotation, [
        LOOT_PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE * (0.5 - u),
        LOOT_PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE * (0.5 - v),
        0,
      ]));
      const hand = await aimVrHandAt(game, target, 0.35);
      const hit = await game.raycast({
        start: hand.start,
        end: hand.target,
        collision_groups: ["world", "entity", "selectable", "raycast", "ui"],
      });
      assert.equal(
        hit.entity_id,
        proxy.entity_id,
        "the native controller ray must reach the module slot",
      );
      const input = mode === "vr-trigger"
        ? "right_hand.trigger"
        : "right_hand.squeeze";
      await game.input.set(input, 1);
      await game.step({ frames: 6 });
      await game.input.set(input, 0);
      await game.step({ frames: 3 });
    } else {
      await clickUiElement(game, slot);
    }

    const after = (await game.info()).player;
    assert.equal(
      after.stats?.cyber_modules,
      before! + 4,
      "native collection must award exactly the authored four modules",
    );
    assert.equal(
      after.right_hand_entity_id,
      null,
      "modules are collected rather than held",
    );
    assert.deepEqual(
      await game.entities.byTemplate(1398), [], "the module pile must be consumed",
    );
    assert.equal((await game.entities.detail(corpse.id)).outgoing_links.some(
      (link) => link.link_type.startsWith("Contains") && link.target_id === cookie.id,
    ), false, "consumption must remove corpse containment");
    await game.step({ frames: 30 });
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      before! + 4,
      "collection must not award the pile twice after settling",
    );
    await game.screenshot(`cyber-module-loot-${mode}-after.png`);
    const save = `cyber_module_loot_${mode}_${Date.now()}`;
    await game.save(save);
    await game.load(save);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      before! + 4,
      "the exact module balance must round-trip",
    );
    assert.deepEqual(
      await game.entities.byTemplate(1398),
      [],
      "the consumed pile must stay consumed after reload",
    );
  });
}
