import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  carriedNaniteTotal,
  physicallyOpenEarthReplicator,
} from "./helpers/earth-replicator.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { clickUiElement } from "./helpers/ui.js";

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
//
// Since #1118, every world nanite pickup collects straight into the player
// stat rather than a carried StackCount entity, so this scenario (built on
// ordinary world pickups) only ever spends from the stat - it can no longer
// reach the legacy stat-then-carried-stack crossing that
// script_util::debit_player_nanites also handles (pre-existing saves,
// panel-taken piles). That crossing, including a carried stack that lands on
// exactly zero and must be destroyed, is covered at the unit level instead:
// script_util::tests::live_nanite_debit_crosses_from_the_stat_into_a_carried_stack_and_exhausts_it.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_NANITE_REWARD = 542;
const EARTH_REPLICATOR = 262;
const EARTH_REPLICATOR_OUTPUT = 284;
const CHIPS_TEMPLATE = -92;

test(
  "Earth replicator charges authored prices and refuses unaffordable output",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
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

    await physicallyOpenEarthReplicator(game, replicator);
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
    // Honor the active localized catalog wording (which may omit a count).
    // The actual purchase below still verifies the child's six-round stack.
    for (const expected of [
      "003", "004", "040", "0270",
      "Bag of chips", "Bottle of juice", "Standard bullets",
    ]) {
      assert.ok(
        text.includes(expected),
        `replicator should visibly render ${expected}; got ${JSON.stringify(text)}`,
      );
    }

    const chipsBefore = new Set(
      (await game.entities.byTemplate(CHIPS_TEMPLATE)).map((entity) => entity.id),
    );
    await clickUiElement(game, chipsButton);
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
    await clickUiElement(game, close);
    await earthWorldUse(game, dispensedChips);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === dispensedChips.id,
      ),
      "the physically dispensed Chips should be collectible normally",
    );

    await physicallyOpenEarthReplicator(game, replicator);
    // Spend down the pooled nanite stat. After Chips, 267 - 6*40 - 2*4 = 19;
    // both nanite piles were collected straight into the stat balance
    // (nothing is left as a carried inventory entity), so every purchase
    // debits that one counter rather than crossing physical stacks.
    for (let purchase = 0; purchase < 6; purchase++) {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `panel should remain open before clip purchase ${purchase + 1}`);
      const clip = panel.elements.find(
        (element) => element.label === "buy:small standard clip",
      );
      assert.ok(clip, "Standard Clip row should remain available");
      const beforeClips = new Set(
        (await game.entities.byTemplate(-1358)).map((entity) => entity.id),
      );
      await clickUiElement(game, clip);
      const purchasedClip = (await game.entities.byTemplate(-1358)).find(
        (entity) => !beforeClips.has(entity.id),
      );
      assert.ok(purchasedClip, "purchase should create Small Standard Clip");
      assert.equal(
        Number((await game.entities.detail(purchasedClip.id)).properties.find(
          (property) => property.name === "StackCount",
        )?.value),
        6,
        "Small Standard Clip must override its parent's twelve-round stack",
      );
    }
    for (let purchase = 0; purchase < 2; purchase++) {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `panel should remain open before Juice purchase ${purchase + 1}`);
      const juice = panel.elements.find(
        (element) => element.label === "buy:juice bottle",
      );
      assert.ok(juice, "Juice row should remain available");
      await clickUiElement(game, juice);
    }
    assert.equal(
      await carriedNaniteTotal(game),
      19,
      "successful purchases should each deduct exactly their authored price",
    );
    const inventoryAfterStatPayment = await game.player.inventory();
    const remainingNanites = inventoryAfterStatPayment.items.filter(
      (item) => item.name?.toLowerCase().includes("nanite"),
    );
    assert.equal(
      remainingNanites.length,
      0,
      "nanites are collected straight into the player stat, never left as a carried inventory entity",
    );
    assert.equal(
      (await game.info()).player.stats?.nanites,
      19,
      "the pooled stat balance should hold the exact remainder",
    );
    assert.deepEqual(
      new Set(
        inventoryAfterStatPayment.items.map((item) => item.entity_id),
      ),
      new Set([dispensedChips.id]),
      "only the physically collected Chips should be carried",
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
    await clickUiElement(game, unaffordableClip);
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
