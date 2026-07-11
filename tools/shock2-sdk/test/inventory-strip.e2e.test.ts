import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the Tab metagame ("use") mode inventory strip
// (projects/flat-ui.md §5.2 / §6 PR 4):
//
//  1. Tab (`ToggleUseMode`) flips /v1/ui mode to "use" AND renders the
//     player's actual carried inventory in the top-docked strip
//     (`internal_inventory`'s ContainerGui, original 636x121 INVBACK anchored
//     top-centered - shkinv.cpp INV_X=2/INV_Y=0).
//  2. /v1/ui exposes the strip's elements with item labels + entity ids -
//     the same introspection contract as MFD panels.
//  3. Movement keys keep working in use mode (only mouse-look is
//     surrendered - manual p.7).
//  4. Clicking a carried weapon in the strip wields it (the flat grab/wield
//     path), and the strip updates live (the wielded item leaves the grid).
//  5. Frobbed MFD panels (loot) coexist with the strip in use mode.
//  6. Tab again restores shooter mode: strip + elements gone.
//
// Item acquisition is honest where it matters: the Wrench is looted from MS
// Male Corpse 1177 via the PR 3 loot panel (the authored first MedSci
// interaction); the Nanites use the debug give lever as test setup.
//
// Negative-first: on main, Tab already flips /v1/ui mode to "use" (the PR 1
// mode flag) but NO inventory strip exists - `strip` is absent/null and no
// carried-item elements appear anywhere. The KEY assertion ("use mode
// exposes a strip listing the carried Wrench") fails on main.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_WRENCH = 1177; // MS Male Corpse -> Contains -> Wrench (mission id 990)

test(
  "Tab metagame mode renders the carried inventory in the top-docked strip",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8133),
    });
    await game.step({ frames: 5 });

    const clickElement = async (el: UiElement) => {
      const [x, y, w, h] = el.screen_rect;
      const center: [number, number] = [x + w / 2, y + h / 2];
      await game.input.set("pointer.position", center);
      await game.step({ frames: 2 }); // hover registers (edge detection needs a prior unpressed frame)
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };

    // --- Setup: give the player a couple of items ---
    // Nanites via the debug give lever (plain test setup)...
    const { entities: nanites } = await game.entities.list({
      filter: "*Nanites*",
      limit: 10,
    });
    assert.ok(nanites[0], "expected a Nanites pickup in medsci1");
    await game.player.give(nanites[0].id);

    // ...and the Wrench looted honestly through the PR 3 loot MFD.
    const corpses = await game.entities.byTemplate(CORPSE_WRENCH);
    assert.equal(corpses.length, 1, "expected exactly one MS Male Corpse (1177)");
    const corpse = corpses[0];
    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const lootPanel = (await game.ui.state()).active_panel;
    assert.ok(lootPanel, "frobbing corpse 1177 should open its loot panel");
    const wrenchLoot = lootPanel.elements.find(
      (e) => e.kind === "button" && e.label === "Wrench",
    );
    assert.ok(wrenchLoot, "loot panel should list the Wrench");
    await clickElement(wrenchLoot);
    const carried = await game.player.inventory();
    const wrench = carried.items.find((i) => i.name === "Wrench");
    assert.ok(wrench, `looting should put the Wrench in the backpack (got ${JSON.stringify(carried.items)})`);
    // Close the loot panel (host close button) so the strip test starts clean.
    const closeBtn = (await game.ui.state()).active_panel?.elements.find(
      (e) => e.label === "close",
    );
    assert.ok(closeBtn, "loot panel should expose the host close button");
    await clickElement(closeBtn);
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "the loot panel should close before the strip test",
    );

    // --- Shooter baseline: no strip ---
    const shooter = await game.ui.state();
    assert.equal(shooter.mode, "shooter");
    assert.ok(!shooter.strip, "shooter mode must not expose an inventory strip");
    await game.screenshot("strip-shooter-before.png");

    // --- KEY: Tab -> use mode with the carried items visible in the strip ---
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    const use = await game.ui.state();
    assert.equal(use.mode, "use", "Tab should flip /v1/ui mode to use");
    assert.ok(
      use.strip,
      `use mode must expose the top-docked inventory strip (got ${JSON.stringify(use)})`,
    );
    const stripWrench = use.strip.elements.find(
      (e) => e.kind === "button" && e.label === "Wrench",
    );
    assert.ok(
      stripWrench,
      `the strip must list the carried Wrench ` +
        `(got ${JSON.stringify(use.strip.elements)})`,
    );
    assert.equal(
      stripWrench.entity_id,
      wrench.entity_id,
      "the strip's Wrench element should carry the item's entity id",
    );
    const stripNanites = use.strip.elements.find(
      (e) => e.kind === "button" && (e.label ?? "").includes("Nanite"),
    );
    assert.ok(
      stripNanites,
      `the strip must list the carried Nanites (got ${JSON.stringify(use.strip.elements)})`,
    );
    // The strip is top-docked: the INVBACK backdrop sits at the canvas top
    // (shkinv.cpp: INV_Y = 0, horizontally centered).
    const backdrop = use.strip.elements.find((e) =>
      (e.texture ?? "").toLowerCase().includes("invback"),
    );
    assert.ok(backdrop, "the strip should draw the INVBACK backdrop");
    assert.equal(backdrop.rect[1], 0, "the strip is anchored at the canvas top");
    await game.screenshot("strip-use-mode.png");

    // --- Movement keys keep working in use mode ---
    const posBefore = await game.player.position();
    await game.input.set("right_hand.thumbstick", [0.0, 1.0]);
    await game.step({ frames: 30 });
    await game.input.set("right_hand.thumbstick", [0.0, 0.0]);
    const posAfter = await game.player.position();
    const moved = Math.hypot(posAfter.x - posBefore.x, posAfter.z - posBefore.z);
    assert.ok(
      moved > 0.1,
      `locomotion must keep working in use mode (moved ${moved.toFixed(3)})`,
    );

    // --- Frobbed panels coexist with the strip in use mode ---
    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const withPanel = await game.ui.state();
    assert.ok(
      withPanel.active_panel,
      "a frobbed loot panel should open while in use mode",
    );
    assert.ok(
      withPanel.strip,
      "the inventory strip should stay up alongside an open MFD panel",
    );
    await game.screenshot("strip-with-panel.png");
    const closeBtn2 = withPanel.active_panel.elements.find((e) => e.label === "close");
    assert.ok(closeBtn2, "the coexisting panel keeps its close button");
    await clickElement(closeBtn2);
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "closing the panel leaves use mode (and the strip) active",
    );
    assert.ok((await game.ui.state()).strip, "the strip survives the panel closing");

    // NOTE: strip item interaction (lift onto the cursor / place / throw) is
    // the cursor-is-the-item drag, covered end-to-end by
    // inventory-drag.e2e.test.ts (flat UI 4.5). This test stays focused on the
    // strip rendering the carried inventory, coexisting with a panel, and Tab
    // toggling the mode.

    // --- Tab again -> shooter restored, strip gone ---
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const restored = await game.ui.state();
    assert.equal(restored.mode, "shooter", "Tab again should restore shooter mode");
    assert.ok(!restored.strip, "leaving use mode must drop the strip");
    await game.screenshot("strip-shooter-after.png");
  },
);
