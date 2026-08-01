import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { clickUiElement } from "./helpers/ui.js";

// Hydroponics' authored Toxin-A research objective. The test deliberately
// starts the vial through the production flat inventory interaction (Tab/use
// mode, cursor lift, second-click use), rather than injecting a script message.
// Runtime entity ids are discovered every launch.
//
// Negative-first evidence for #763: on origin/main at 5ebca74d the second
// inventory click reaches the `ResearchableScript` stub, leaving
// `/v1/ui.active_panel` null. The first panel assertion below therefore fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function useInventoryItem(
  game: GameServer,
  entityId: number,
): Promise<void> {
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use", "inventory use requires the live use-mode strip");
  const element = ui.strip?.elements.find(
    (candidate: UiElement) => candidate.entity_id === entityId,
  );
  assert.ok(element, `strip should expose carried item ${entityId}`);

  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor?.entity_id,
    entityId,
    "first click should lift the item onto the cursor",
  );
  await clickUiElement(game, element);
  assert.equal(
    (await game.ui.state()).cursor,
    null,
    "second click should use the item and clear the cursor",
  );
  await game.step({ frames: 5 });
}

test(
  "hydro2.mis: Toxin-A starts research through the real flat inventory",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8233),
    });
    await game.step({ frames: 5 });

    const toxin = (
      await game.entities.list({ filter: "Anti-Annelid Toxin", limit: 20 })
    ).entities[0];
    assert.ok(toxin, "Hydro 2 should contain an authored Toxin-A vial");
    await game.player.give(toxin.id);

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await useInventoryItem(game, toxin.id);

    const opened = await game.ui.state();
    assert.ok(
      opened.active_panel,
      "using unresearched Toxin-A should open the research panel",
    );
    assert.equal(opened.active_panel.entity_id, toxin.id);
    assert.ok(
      opened.active_panel.elements.some(
        (element) => element.texture?.toLowerCase() === "research.pcx",
      ),
      "the panel should use the retail RESEARCH.PCX backdrop",
    );
  },
);
