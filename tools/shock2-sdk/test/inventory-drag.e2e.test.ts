import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the cursor-is-the-item inventory drag (flat UI 4.5,
// projects/flat-ui.md §1.5 / §2.4 / §5.2):
//
//   - LMB on a strip item LIFTS it onto the cursor (the cursor becomes the
//     item; the slot empties). /v1/ui exposes the held item (`cursor`).
//   - LMB on another slot PLACES it back into the grid.
//   - LMB on the bare 3D view THROWS the held item into the world (it gains
//     physics presence near the player).
//   - Escape hatch: Tab-out with an item on the cursor returns it to the
//     backpack rather than losing it.
//
// Negative-first: on unmodified main the strip click WIELDS the item (the
// interim PR 4 behavior) - there is no cursor-held state, so `/v1/ui.cursor`
// is absent and the KEY assertion ("clicking a strip item lifts it onto the
// cursor") fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_WRENCH = 1177; // MS Male Corpse -> Contains -> Wrench (mission id 990)
const CORPSE_PSI_AMP = 219; // Male Corpse 1 -> Contains -> Psi Amp (mission id 1407)

test(
  "cursor-is-the-item: lift, place, wield (double-click), and throw a strip item",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8156),
    });
    await game.step({ frames: 5 });

    const setPointer = async (screen: [number, number], pressed: 0 | 1) => {
      await game.input.set("pointer.position", screen);
      await game.input.set("pointer.pressed", pressed);
    };
    const center = (el: UiElement): [number, number] => {
      const [x, y, w, h] = el.screen_rect;
      return [x + w / 2, y + h / 2];
    };
    // A discrete click at a screen point: hover (an unpressed frame so the
    // press is an edge), press, release.
    const clickAt = async (screen: [number, number]) => {
      await setPointer(screen, 0);
      await game.step({ frames: 2 });
      await setPointer(screen, 1);
      await game.step({ frames: 2 });
      await setPointer(screen, 0);
      await game.step({ frames: 2 });
    };
    const clickElement = (el: UiElement) => clickAt(center(el));

    // Loot an item honestly through a PR-3 loot MFD (frob corpse -> take).
    const lootFromCorpse = async (template: number, itemName: string) => {
      const corpses = await game.entities.byTemplate(template);
      assert.equal(corpses.length, 1, `expected exactly one corpse ${template}`);
      const corpse = corpses[0];
      await teleportVerified(game, {
        x: corpse.position[0] + 1.0,
        y: corpse.position[1] + 0.5,
        z: corpse.position[2] + 1.0,
      });
      await game.entities.sendMessage(corpse.id, { type: "Frob" });
      await game.step({ frames: 5 });
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `frobbing corpse ${template} should open its loot panel`);
      const loot = panel.elements.find(
        (e) => e.kind === "button" && e.label === itemName,
      );
      assert.ok(loot, `loot panel should list ${itemName}`);
      await clickElement(loot);
      // Close the loot panel (bare-view click) so the next step starts clean.
      const close = (await game.ui.state()).active_panel?.elements.find(
        (e) => e.label === "close",
      );
      if (close) await clickElement(close);
    };

    const stripItem = (ui: UiState, name: string): UiElement | undefined =>
      ui.strip?.elements.find((e) => e.kind === "button" && e.label === name);

    // --- Setup: two grabbable items in the backpack ---
    await lootFromCorpse(CORPSE_WRENCH, "Wrench");
    await lootFromCorpse(CORPSE_PSI_AMP, "Psi Amp");
    const carried = await game.player.inventory();
    const wrench = carried.items.find((i) => i.name === "Wrench");
    const amp = carried.items.find((i) => i.name === "Psi Amp");
    assert.ok(wrench && amp, `both items should be carried (got ${JSON.stringify(carried.items)})`);

    // --- Tab into use mode; strip lists both items ---
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    let ui = await game.ui.state();
    assert.equal(ui.mode, "use");
    assert.ok(stripItem(ui, "Wrench"), "strip should list the Wrench");
    assert.ok(stripItem(ui, "Psi Amp"), "strip should list the Psi Amp");
    assert.ok(!ui.cursor, "nothing is on the cursor before lifting");
    const stripBackdrop = ui.strip!.elements.find((e) =>
      (e.texture ?? "").toLowerCase().includes("invback"),
    )!;
    await game.screenshot("drag-before-lift.png");

    // --- KEY: a single LMB on the Wrench LIFTS it onto the cursor (it does
    // NOT wield - that is the double-click, tested below) ---
    const wrenchEl = stripItem(ui, "Wrench")!;
    await clickElement(wrenchEl);
    ui = await game.ui.state();
    assert.ok(
      ui.cursor,
      `a single click on a strip item must lift it onto the cursor (got ${JSON.stringify(ui)})`,
    );
    assert.equal(ui.cursor.entity_id, wrench!.entity_id, "the cursor holds the Wrench entity");
    assert.equal(ui.cursor.label, "Wrench", "the cursor is labeled with the item");
    assert.ok(!stripItem(ui, "Wrench"), "the lifted Wrench leaves the strip grid");
    assert.ok(stripItem(ui, "Psi Amp"), "the Psi Amp stays in the strip");
    // A single click lifts, it must NOT wield: no item is in hand.
    const liftedInv = await game.player.inventory();
    assert.ok(
      !liftedInv.items.some((i) => i.location === "left_hand" || i.location === "right_hand"),
      `a single click must not wield (got ${JSON.stringify(liftedInv.items)})`,
    );
    await game.screenshot("drag-lifted.png");

    // --- PLACE it back: LMB on an empty strip cell (backdrop center) ---
    await clickAt(center(stripBackdrop));
    ui = await game.ui.state();
    assert.ok(!ui.cursor, "placing clears the cursor");
    assert.ok(stripItem(ui, "Wrench"), "the placed Wrench returns to the strip grid");
    await game.screenshot("drag-placed.png");

    // --- Escape hatch: lift the Wrench, Tab-out returns it, then re-enter ---
    await clickElement(stripItem(ui, "Wrench")!);
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      wrench!.entity_id,
      "the Wrench is on the cursor",
    );
    await game.input.trigger("ToggleUseMode"); // Tab-out
    await game.step({ frames: 5 });
    let shooter = await game.ui.state();
    assert.equal(shooter.mode, "shooter", "Tab-out returns to shooter mode");
    assert.ok(!shooter.cursor, "leaving use mode clears the cursor");
    const returned = await game.player.inventory();
    const returnedWrench = returned.items.find((i) => i.name === "Wrench");
    assert.ok(
      returnedWrench && returnedWrench.location === "inventory",
      `Tab-out with an item on the cursor must return it to the backpack ` +
        `(got ${JSON.stringify(returned.items)})`,
    );
    await game.input.trigger("ToggleUseMode"); // Tab back in
    await game.step({ frames: 5 });

    // --- WIELD: a DOUBLE-click on the Wrench equips it (single=lift,
    // double=wield - the faithful single-button mapping) ---
    const wrenchEl2 = stripItem(await game.ui.state(), "Wrench")!;
    await clickElement(wrenchEl2); // first click lifts...
    await clickElement(wrenchEl2); // ...quick second click on the same slot wields
    ui = await game.ui.state();
    assert.ok(!ui.cursor, "wielding clears the cursor");
    const afterWield = await game.player.inventory();
    const wieldedWrench = afterWield.items.find((i) => i.name === "Wrench");
    assert.equal(
      wieldedWrench?.location,
      "left_hand",
      `double-clicking a carried weapon should wield it (got ${JSON.stringify(afterWield.items)})`,
    );
    assert.ok(!stripItem(ui, "Wrench"), "the wielded Wrench leaves the strip grid");
    await game.screenshot("drag-wielded.png");

    // --- THROW: lift the Psi Amp, then LMB on the bare 3D view throws it ---
    await clickElement(stripItem(await game.ui.state(), "Psi Amp")!);
    ui = await game.ui.state();
    assert.equal(ui.cursor?.entity_id, amp!.entity_id, "the Psi Amp is on the cursor");
    const playerBefore = await game.player.position();
    // A point below the top-docked strip (y > 121 canvas) and clear of the
    // bottom HUD readouts: dead center-ish of the 3D view.
    await clickAt([0.5, 0.6]);
    ui = await game.ui.state();
    assert.ok(!ui.cursor, "throwing clears the cursor");
    assert.ok(!stripItem(ui, "Psi Amp"), "the thrown Psi Amp is gone from the strip");
    const afterThrow = await game.player.inventory();
    assert.ok(
      !afterThrow.items.some((i) => i.name === "Psi Amp"),
      "the thrown Psi Amp is no longer carried",
    );
    // The thrown item gained world presence near the player.
    const bodies = await game.physics.bodies({ entityId: amp!.entity_id });
    assert.ok(
      bodies.bodies.length > 0,
      `the thrown Psi Amp should have a physics body (got ${JSON.stringify(bodies.bodies)})`,
    );
    const p = bodies.bodies[0].position;
    assert.ok(
      Number.isFinite(p[0]) && Number.isFinite(p[1]) && Number.isFinite(p[2]),
      `the thrown Psi Amp body position must be finite (got ${JSON.stringify(p)})`,
    );
    const dist = Math.hypot(
      p[0] - playerBefore.x,
      p[1] - playerBefore.y,
      p[2] - playerBefore.z,
    );
    assert.ok(dist < 10, `the thrown Psi Amp should land near the player (dist ${dist.toFixed(2)})`);
    await game.screenshot("drag-thrown.png");
  },
);
