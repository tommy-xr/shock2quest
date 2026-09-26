import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanel } from "../src/types.js";
import {
  carriedNaniteTotal,
  physicallyOpenEarthReplicator,
} from "./helpers/earth-replicator.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { clickUiElement } from "./helpers/ui.js";

// Honest Earth Technical Training regression for #544. Runtime ids are
// rediscovered after every load; positive ids below are stable authored
// mission objects. Teleport only stages a clear camera position. Nanite pickup,
// replicator frob, sidecar/HRM input, purchase, and output collection all use
// ordinary rendered-world/player input.
//
// Negative-first evidence (2026-07-23): against exact #543 head cc466f9,
// physical pickup and frob succeed and the normal 003/004/040 inventory is
// visible, but no PLUGHACK sidecar or `hack-replicator` action exists.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_REPLICATOR = 262;
const EARTH_REPLICATOR_OUTPUT = 284;

function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(found, `panel should expose button ${label}`);
  return found;
}

function texts(panel: UiPanel): string[] {
  return panel.elements
    .filter((element) => element.kind === "text")
    .map((element) => element.text ?? "");
}

async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "replicator interaction should keep an MFD panel open");
  return panel;
}

test(
  "Earth replicator: hack sidecar, paid HRM win, hacked purchase, save/load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [replicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    const [output] = await game.entities.byTemplate(EARTH_REPLICATOR_OUTPUT);
    assert.ok(nanites, "Earth should contain authored nanite pile 257");
    assert.ok(replicator, "Earth should contain authored replicator 262");
    assert.ok(output, "Earth should contain authored output marker 284");

    await earthWorldUse(game, nanites);
    assert.equal(
      await carriedNaniteTotal(game),
      250,
      "physical pickup should carry the authored 250-nanite stack",
    );

    await physicallyOpenEarthReplicator(game, replicator);
    const normal = await activePanel(game);
    assert.equal(normal.template_id, EARTH_REPLICATOR);
    assert.ok(
      button(normal, "buy:chips"),
      "unhacked replicator should retain its normal inventory",
    );
    assert.ok(
      normal.elements.some(
        (element) => element.texture?.toLowerCase() === "plughack.pcx",
      ),
      "unhacked HackDiff replicator should render the original PLUGHACK sidecar",
    );

    await clickUiElement(game, button(normal, "hack-replicator"));
    const boardBeforePayment = await activePanel(game);
    assert.ok(
      boardBeforePayment.elements.some(
        (element) => element.texture?.toLowerCase() === "hack.pcx",
      ),
      "sidecar action should open the shared retail HRM board",
    );
    const authoredCost = texts(boardBeforePayment).find((text) => /^\d+$/.test(text));
    assert.equal(authoredCost, "3", "Earth RepBase should expose authored hack cost 3");

    await clickUiElement(game, button(boardBeforePayment, "start-hack"));
    assert.equal(
      await carriedNaniteTotal(game),
      247,
      "starting the real board should charge exactly the authored hack cost",
    );

    // Try the compact top-row route first, but react to the actual outcome:
    // the authored board may burn its third top node. The same live board then
    // has a vertical continuation through x=2. This is genuine HRM play, not a
    // direct success message or an assumption that any one roll must win.
    for (const label of [
      "node-2-0",
      "node-3-0",
      "node-4-0",
      "node-2-1",
      "node-2-2",
      "node-2-3",
    ]) {
      const current = await activePanel(game);
      if (
        current.elements.some(
          (element) => element.texture?.toLowerCase() === "winh.pcx",
        )
      ) {
        break;
      }
      await clickUiElement(game, button(current, label));
    }
    const won = await activePanel(game);
    assert.ok(
      won.elements.some(
        (element) => element.texture?.toLowerCase() === "winh.pcx",
      ),
      `connected-three should win the real HRM board; rng=${game
        .logs()
        .filter((line) => line.includes("HRM rng"))
        .join(" | ")}`,
    );

    await clickUiElement(game, button(won, "close"));
    await physicallyOpenEarthReplicator(game, replicator);
    const hacked = await activePanel(game);
    assert.ok(
      texts(hacked).includes("Standard bullets"),
      "hacked catalog must use the active localized Small Standard Clip name",
    );
    assert.ok(
      texts(hacked).every((text) => !text.includes("%d") && !text.includes('"')),
      "hacked catalog must not expose raw property strings or quantity tokens",
    );
    for (const label of [
      "buy:small he clip",
      "buy:small standard clip",
      "buy:med patch",
    ]) {
      assert.ok(button(hacked, label), `hacked inventory should expose ${label}`);
    }
    for (const expected of ["070", "025", "020", "0247"]) {
      assert.ok(
        texts(hacked).includes(expected),
        `hacked inventory should render ${expected}; got ${JSON.stringify(texts(hacked))}`,
      );
    }
    assert.ok(
      !hacked.elements.some(
        (element) =>
          element.label === "hack-replicator" ||
          element.texture?.toLowerCase() === "plughack.pcx",
      ),
      "a hacked replicator should not offer the hack sidecar again",
    );

    const heClipsBefore = new Set(
      (
        await game.entities.list({
          filter: "*HE Clip*",
          limit: 100,
        })
      ).entities.map((entity) => entity.id),
    );
    await clickUiElement(game, button(hacked, "buy:small he clip"));
    await game.step({ frames: 20 });
    assert.equal(
      await carriedNaniteTotal(game),
      177,
      "hacked purchase should use the authored 70-nanite price",
    );
    const dispensed = (
      await game.entities.list({
        filter: "*HE Clip*",
        limit: 100,
      })
    ).entities.find((entity) => !heClipsBefore.has(entity.id));
    assert.ok(dispensed, "successful hacked purchase should create Small HE Clip");
    const outputPosition = (await game.entities.detail(output.id)).position;
    const itemPosition = (await game.entities.detail(dispensed.id)).position;
    assert.ok(
      Math.hypot(
        itemPosition[0] - outputPosition[0],
        itemPosition[1] - outputPosition[1],
        itemPosition[2] - outputPosition[2],
      ) < 2,
      "hacked purchase should appear at authored output marker 284",
    );

    await clickUiElement(game, button(await activePanel(game), "close"));
    await earthWorldUse(game, dispensed);
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) => item.entity_id === dispensed.id,
      ),
      "physically dispensed hacked item should be collectible",
    );

    const saveName = `earth_replicator_hacked_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    assert.equal(
      await carriedNaniteTotal(game),
      177,
      "post-hack nanite balance should survive save/load",
    );
    assert.ok(
      (await game.player.inventory()).items.some((item) =>
        item.name?.toLowerCase().includes("he clip"),
      ),
      "physically collected hacked item should survive save/load",
    );

    const [loadedReplicator] = await game.entities.byTemplate(EARTH_REPLICATOR);
    assert.ok(loadedReplicator, "save/load should restore authored replicator 262");
    await physicallyOpenEarthReplicator(game, loadedReplicator);
    const loadedHacked = await activePanel(game);
    assert.ok(
      button(loadedHacked, "buy:small he clip"),
      "loaded replicator should reopen with hacked inventory",
    );
    for (const expected of ["070", "025", "020", "0177"]) {
      assert.ok(
        texts(loadedHacked).includes(expected),
        `loaded hacked panel should render ${expected}; got ${JSON.stringify(texts(loadedHacked))}`,
      );
    }
    assert.ok(
      !loadedHacked.elements.some(
        (element) =>
          element.label === "hack-replicator" ||
          element.texture?.toLowerCase() === "plughack.pcx",
      ),
      "persistent Hacked state should suppress the sidecar after load",
    );
  },
);
