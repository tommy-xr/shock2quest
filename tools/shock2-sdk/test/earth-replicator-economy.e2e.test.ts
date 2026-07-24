import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement } from "../src/types.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { teleportVerified } from "./helpers/teleport.js";

// Honest Earth Technical Training regression for #543. Runtime entity ids are
// rediscovered every launch; the positive ids are stable authored mission
// objects. Setup uses teleport only to stage the camera, while pickup, frob,
// row selection, output collection, and all resulting state changes flow
// through ordinary rendered-world/player input.
//
// Negative-first evidence (2026-07-23): against 89ed730, the physical frob
// opens the replicator and a Chips click creates collectible template -92, but
// the panel exposes neither authored prices nor the carried balance and the
// both real StackCount entities remain unchanged at 250 + 20.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_NANITE_REWARD = 542;
const EARTH_REPLICATOR = 262;
const EARTH_REPLICATOR_OUTPUT = 284;
const CHIPS_TEMPLATE = -92;

async function clickElement(game: GameServer, element: UiElement) {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [
    x + width / 2,
    y + height / 2,
  ]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

async function carriedNaniteTotal(game: GameServer): Promise<number> {
  const inventory = await game.player.inventory();
  let total = 0;
  for (const item of inventory.items) {
    if (!item.name?.toLowerCase().includes("nanite")) continue;
    const detail = await game.entities.detail(item.entity_id);
    const stack = detail.properties.find(
      (property) => property.name === "StackCount",
    );
    assert.ok(stack, `carried nanite entity ${item.entity_id} needs StackCount`);
    total += Number(stack.value);
  }
  return total;
}

async function physicallyOpenReplicator(
  game: GameServer,
  replicator: EntitySummary,
) {
  const [x, _y, z] = (await game.entities.detail(replicator.id)).position;
  // This is the same clear standing point used by the accepted Technical
  // play-through. The fresh Earth body heading makes negative yaw face the
  // machine; a modest upward pitch hits its green interaction screen.
  await teleportVerified(game, { x: x - 1.59, y: 21.404, z: z - 2.23 });
  await game.input.set("head.look", [-35.3, -22]);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
}

test(
  "Earth replicator charges authored prices and refuses unaffordable output",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8156),
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [naniteReward] = await game.entities.byTemplate(EARTH_NANITE_REWARD);
    const [replicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    const [output] = await game.entities.byTemplate(EARTH_REPLICATOR_OUTPUT);
    assert.ok(nanites, "Earth should contain authored nanite pile 257");
    assert.ok(naniteReward, "Earth should contain authored nanite reward 542");
    assert.ok(replicator, "Earth should contain authored replicator 262");
    assert.ok(output, "Earth should contain authored replicator output 284");

    await earthWorldUse(game, nanites);
    await earthWorldUse(game, naniteReward);
    assert.equal(
      await carriedNaniteTotal(game),
      270,
      "physical pickup should carry both authored 250 + 20 nanite stacks",
    );

    await physicallyOpenReplicator(game, replicator);
    const opened = (await game.ui.state()).active_panel;
    assert.ok(opened, "physical replicator frob should open an MFD panel");
    assert.equal(opened.template_id, EARTH_REPLICATOR);

    const chipsButton = opened.elements.find(
      (element) =>
        element.kind === "button" && element.label === "buy:chips",
    );
    const juiceButton = opened.elements.find(
      (element) =>
        element.kind === "button" && element.label === "buy:juice bottle",
    );
    const clipButton = opened.elements.find(
      (element) =>
        element.kind === "button" &&
        element.label === "buy:small standard clip",
    );
    assert.ok(chipsButton, "replicator should expose the authored Chips row");
    assert.ok(juiceButton, "replicator should expose the authored Juice row");
    assert.ok(clipButton, "replicator should expose the authored Standard Clip row");

    const text = opened.elements
      .filter((element) => element.kind === "text")
      .map((element) => element.text ?? "");
    for (const expected of ["003", "004", "040", "0270"]) {
      assert.ok(
        text.includes(expected),
        `replicator should visibly render ${expected}; got ${JSON.stringify(text)}`,
      );
    }

    const chipsBefore = new Set(
      (await game.entities.byTemplate(CHIPS_TEMPLATE)).map((entity) => entity.id),
    );
    await clickElement(game, chipsButton);
    await game.step({ frames: 20 });
    assert.equal(
      await carriedNaniteTotal(game),
      267,
      "selecting Chips should deduct its authored 3-nanite price",
    );
    const afterChipsText = (await game.ui.state()).active_panel?.elements
      .filter((element) => element.kind === "text")
      .map((element) => element.text ?? "");
    assert.ok(
      afterChipsText?.includes("0267"),
      `the visible balance should update after payment; got ${JSON.stringify(afterChipsText)}`,
    );

    const dispensedChips = (await game.entities.byTemplate(CHIPS_TEMPLATE)).find(
      (entity) => !chipsBefore.has(entity.id),
    );
    assert.ok(dispensedChips, "successful payment should create Chips");
    const outputPosition = (await game.entities.detail(output.id)).position;
    const chipsPosition = (await game.entities.detail(dispensedChips.id)).position;
    assert.ok(
      Math.hypot(
        chipsPosition[0] - outputPosition[0],
        chipsPosition[1] - outputPosition[1],
        chipsPosition[2] - outputPosition[2],
      ) < 2,
      "the purchase should appear at authored output marker 284",
    );

    const close = (await game.ui.state()).active_panel?.elements.find(
      (element) => element.label === "close",
    );
    assert.ok(close, "replicator panel should expose the ordinary close control");
    await clickElement(game, close);
    await earthWorldUse(game, dispensedChips);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === dispensedChips.id,
      ),
      "the physically dispensed Chips should be collectible normally",
    );

    await physicallyOpenReplicator(game, replicator);
    // Spend through both carried StackCounts. After Chips, 267 - 6*40 - 2*4
    // leaves 19; the second Juice crosses the remaining balance of the first
    // stack, so exact payment must remove that exhausted entity and continue
    // into the other real stack.
    for (let purchase = 0; purchase < 6; purchase++) {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `panel should remain open before clip purchase ${purchase + 1}`);
      const clip = panel.elements.find(
        (element) => element.label === "buy:small standard clip",
      );
      assert.ok(clip, "Standard Clip row should remain available");
      await clickElement(game, clip);
    }
    for (let purchase = 0; purchase < 2; purchase++) {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `panel should remain open before Juice purchase ${purchase + 1}`);
      const juice = panel.elements.find(
        (element) => element.label === "buy:juice bottle",
      );
      assert.ok(juice, "Juice row should remain available");
      await clickElement(game, juice);
    }
    assert.equal(
      await carriedNaniteTotal(game),
      19,
      "successful purchases should each deduct exactly their authored price",
    );
    const inventoryAfterCrossStackPayment = await game.player.inventory();
    const remainingNanites = inventoryAfterCrossStackPayment.items.filter(
      (item) => item.name?.toLowerCase().includes("nanite"),
    );
    assert.equal(
      remainingNanites.length,
      1,
      "cross-stack payment should remove the exhausted nanite entity",
    );
    const remainingStack = (
      await game.entities.detail(remainingNanites[0].entity_id)
    ).properties.find((property) => property.name === "StackCount");
    assert.equal(
      Number(remainingStack?.value),
      19,
      "cross-stack payment should leave the exact remainder on the live stack",
    );
    assert.equal(
      inventoryAfterCrossStackPayment.count,
      2,
      "destroyed stack cleanup must remove its Contains link before a dispensed entity can recycle the id",
    );
    assert.deepEqual(
      new Set(
        inventoryAfterCrossStackPayment.items.map((item) => item.entity_id),
      ),
      new Set([dispensedChips.id, remainingNanites[0].entity_id]),
      "only the physically collected Chips and live nanite remainder should be carried",
    );

    const clipsBeforeRefusal = await game.entities.list({
      filter: "*Standard Clip*",
      limit: 100,
    });
    const refusalPanel = (await game.ui.state()).active_panel;
    assert.ok(refusalPanel, "panel should remain open before refused purchase");
    const unaffordableClip = refusalPanel.elements.find(
      (element) => element.label === "buy:small standard clip",
    );
    assert.ok(unaffordableClip, "unaffordable row should remain inspectable");
    await clickElement(game, unaffordableClip);
    await game.step({ frames: 20 });

    assert.equal(
      await carriedNaniteTotal(game),
      19,
      "an unaffordable selection must not debit any partial payment",
    );
    const clipsAfterRefusal = await game.entities.list({
      filter: "*Standard Clip*",
      limit: 100,
    });
    assert.equal(
      clipsAfterRefusal.entities.length,
      clipsBeforeRefusal.entities.length,
      "an unaffordable selection must not mint a free output item",
    );
    const refused = (await game.ui.state()).active_panel;
    assert.ok(
      refused?.elements.some(
        (element) =>
          element.kind === "text" &&
          element.text === "Insufficient nanites",
      ),
      "the visible panel should explain the refusal",
    );
  },
);
