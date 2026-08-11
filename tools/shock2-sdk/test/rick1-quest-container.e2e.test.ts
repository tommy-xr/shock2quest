import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";

// This regression depends on a deep campaign save, which contains player state
// and retail-derived mission data and therefore cannot live in the repository.
// Opt in by naming the locally preserved save (without `.sav`):
//
//   SHOCK2_RICK1_CARD_CORPSE_SAVE=repro-rick1-card-corpse-before-frob \
//     SHOCK2_E2E=1 node --test dist/test/rick1-quest-container.e2e.test.js
const saveName = process.env.SHOCK2_RICK1_CARD_CORPSE_SAVE;
const e2eEnabled = process.env.SHOCK2_E2E === "1" && saveName !== undefined;

// Stable rick1 mission-object ids. Runtime entity ids are rediscovered after
// every launch/load and are never hardcoded.
const QUEST_CORPSE = 1636;
const RICKENBACKER_CARD = 1779;

const containsLinks = async (game: GameServer, entityId: number) =>
  (await game.entities.detail(entityId)).outgoing_links.filter((link) =>
    link.link_type.startsWith("Contains"),
  );

const clickElement = async (game: GameServer, element: UiElement) => {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [x + width / 2, y + height / 2]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 5 });
};

test(
  "rick1 exact save: FrobQB quest corpse keeps its Container MFD and loot",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rick1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8523),
    });

    const loaded = await game.load(saveName!);
    assert.equal(loaded.success, true);
    assert.equal(loaded.mission, "rick1.mis");
    await game.step({ frames: 5 });

    const [corpse] = await game.entities.byTemplate(QUEST_CORPSE);
    const [card] = await game.entities.byTemplate(RICKENBACKER_CARD);
    assert.ok(corpse, "expected rick1 RIC Male Corpse object 1636");
    assert.ok(card, "expected the corpse's Rickenbacker Card object 1779");
    const beforeLinks = await containsLinks(game, corpse.id);
    assert.equal(beforeLinks.length, 4, "the intact corpse should begin with all four loot links");
    assert.ok(
      beforeLinks.some((link) => link.target_id === card.id),
      `corpse 1636 should contain card 1779; got ${JSON.stringify(beforeLinks)}`,
    );
    assert.equal(await game.quests.get("note_7_8"), "unknown");

    const aim = await game.player.aimAt(corpse, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      aim.target_confirmed,
      true,
      `the corpse should be production-targeted before frob: ${JSON.stringify(aim)}`,
    );
    await game.step({ frames: 2 });
    await game.screenshot("rick1-quest-container-before.png");

    // Production interaction: one real squeeze edge dispatches both authored
    // siblings (`ContainerScript` + `FrobQB`) in the same update.
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 5 });

    const opened = await game.ui.state();
    assert.ok(
      opened.active_panel,
      `the corpse's Container MFD must survive the FrobQB effect batch: ${JSON.stringify(opened)}`,
    );
    assert.equal(opened.active_panel.entity_id, corpse.id);
    assert.equal(await game.quests.get("note_7_8"), "complete");

    const [stillCorpse] = await game.entities.byTemplate(QUEST_CORPSE);
    assert.ok(stillCorpse, "FrobQB must not destroy the sibling-owned corpse host");
    assert.equal(stillCorpse.id, corpse.id);
    const afterFrobLinks = await containsLinks(game, stillCorpse.id);
    assert.equal(
      afterFrobLinks.length,
      4,
      "opening the quest container must preserve all four Contains links",
    );
    assert.ok(afterFrobLinks.some((link) => link.target_id === card.id));

    const cardElement = opened.active_panel.elements.find(
      (element) => element.kind === "button" && element.entity_id === card.id,
    );
    assert.ok(
      cardElement,
      `the intact Container MFD should expose card 1779; got ${JSON.stringify(opened.active_panel.elements)}`,
    );
    await game.screenshot("rick1-quest-container-open.png");

    // Taking the card is deliberately the existing ordinary Container MFD
    // path. Logical key-access acquisition is tracked separately by #583 / PR
    // #743; this regression owns host lifetime, panel stability, and physical
    // loot transfer only.
    await clickElement(game, cardElement);
    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === card.id)?.location,
      "inventory",
      `the physical card should transfer normally: ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      (await containsLinks(game, corpse.id)).length,
      3,
      "taking only the card should leave the corpse and its other three loot links intact",
    );
    await game.screenshot("rick1-quest-container-card-taken.png");

    const roundTripSave = `rick1_quest_container_${Date.now()}`;
    assert.equal((await game.save(roundTripSave)).success, true);
    assert.equal((await game.load(roundTripSave)).success, true);
    await game.step({ frames: 5 });

    const [loadedCorpse] = await game.entities.byTemplate(QUEST_CORPSE);
    assert.ok(loadedCorpse, "save/load must retain the corpse container host");
    assert.equal((await containsLinks(game, loadedCorpse.id)).length, 3);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.name === "Rickenbacker Card" && item.location === "inventory",
      ),
      "save/load must retain the physically taken Rickenbacker Card",
    );
    assert.equal(await game.quests.get("note_7_8"), "complete");
  },
);
