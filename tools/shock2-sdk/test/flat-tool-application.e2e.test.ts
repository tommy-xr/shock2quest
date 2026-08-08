import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { clickUiElement } from "./helpers/ui.js";

// Flat tool delivery (#817): while an inventory item rides the use-mode
// cursor, RMB on the bare world view offers it to the crosshair target. LMB
// remains the ordinary throw gesture (covered by inventory-drag.e2e.test.ts).
//
// Negative-first (verified against main, c07099af): the debug runtime rejects
// `pointer.secondary_pressed` because no flat secondary gesture exists. The
// only current bare-view click is LMB, which inventory-drag proves clears the
// cursor and gives the item a world physics body instead of sending
// ProvideForConsumption.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const ICE_PICK = -73;
const SECURITY_CRATE = 325;

function stripItem(ui: UiState, name: string): UiElement {
  const item = ui.strip?.elements.find(
    (element) => element.kind === "button" && element.label === name,
  );
  assert.ok(
    item,
    `inventory strip should contain ${name}; got ${JSON.stringify(
      ui.strip?.elements.map((element) => element.label ?? element.texture),
    )}`,
  );
  return item;
}

async function secondaryClickBareView(game: GameServer): Promise<void> {
  // The center of the world view is below the top inventory strip and outside
  // the left MFD slot. Pulse the secondary button through real pointer input.
  await game.input.set("pointer.position", [0.5, 0.6]);
  await game.input.set("pointer.secondary_pressed", 0);
  await game.step({ frames: 2 });
  await game.input.set("pointer.secondary_pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.secondary_pressed", 0);
  await game.step({ frames: 2 });
}

async function primaryClickBareView(game: GameServer): Promise<void> {
  await game.input.set("pointer.position", [0.5, 0.6]);
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

test(
  "flat cursor applies an ICE Pick to its crosshair target without hijacking rejected items",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9817),
    });
    await game.step({ frames: 5 });

    const [crate] = await game.entities.byTemplate(SECURITY_CRATE);
    assert.ok(crate, "hydro1 should contain security crate 325");
    const [x, y, z] = crate.position;
    await teleportVerified(game, { x: x + 1.2, y: y + 0.5, z: z + 1.2 });
    await game.player.aimAt(crate, { hitbox: "center", visibility: "required" });
    await game.step({ frames: 3 });

    const amp = await game.player.spawnItem("Psi Amp");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    // A target-side refusal must not drop, throw, deposit, or consume the
    // cursor item. The sealed HackableCrate explicitly refuses a Psi Amp.
    let ui = await game.ui.state();
    await clickUiElement(game, stripItem(ui, "Psi Amp"));
    await secondaryClickBareView(game);
    ui = await game.ui.state();
    assert.equal(
      ui.cursor?.entity_id,
      amp.entity_id,
      "a rejected tool must remain on the cursor",
    );
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === amp.entity_id,
      ),
      "a rejected tool must remain in the backpack",
    );

    // LMB remains the ordinary throw path. It clears the cursor, detaches the
    // item from the backpack, and gives it world physics.
    await game.input.set("head.look", [180, 0]);
    await game.step({ frames: 3 });
    await primaryClickBareView(game);
    assert.ok(!(await game.ui.state()).cursor);
    assert.ok(
      (await game.physics.bodies({ entityId: amp.entity_id })).bodies.length > 0,
      "LMB should still throw the cursor item into the world",
    );

    // Reacquire the crate, provision the ICE Pick into the now-free backpack
    // slot, then apply it with RMB. The crate claims FreeHack, becomes Hacked,
    // and destroys the consumed Pick; destruction clears the host cursor.
    await teleportVerified(game, { x: x + 1.2, y: y + 0.5, z: z + 1.2 });
    await game.player.aimAt(crate, { hitbox: "center", visibility: "required" });
    await game.step({ frames: 3 });
    const pick = await game.player.spawnItem(ICE_PICK);
    await game.step({ frames: 5 });
    ui = await game.ui.state();
    await clickUiElement(game, stripItem(ui, "ICE Pick"));
    assert.equal((await game.ui.state()).cursor?.entity_id, pick.entity_id);
    await secondaryClickBareView(game);
    await game.step({ frames: 3 });

    ui = await game.ui.state();
    assert.ok(!ui.cursor, "the consumed ICE Pick should leave the cursor");
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === pick.entity_id,
      ),
      "the crate should consume the ICE Pick",
    );

    // Opening the crate through its normal frob flow proves the target-side
    // state change: Hacked crates show the loot face instead of the hack board.
    await game.entities.sendMessage(crate.id, { type: "Frob" });
    await game.step({ frames: 5 });
    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "the applied ICE Pick should leave the crate openable");
    assert.ok(
      panel.elements.some(
        (element) => element.texture?.toLowerCase() === "contain.pcx",
      ),
      "the ICE Pick should open the crate directly to its loot face",
    );
    assert.ok(
      !panel.elements.some(
        (element) => element.texture?.toLowerCase() === "hack.pcx",
      ),
      "the applied ICE Pick should bypass the hack board",
    );
  },
);
