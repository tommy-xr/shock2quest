import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { acquireOsUpgrade } from "./helpers/os-upgrade.js";

test("Spatially Aware selects the retail full map on the current and next deck", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.step({ frames: 5 });
  async function page(expected: string) {
    await game.input.trigger("ToggleMap");
    await game.step({ frames: 3 });
    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel);
    assert.ok(panel.elements.some(e => e.texture?.toLowerCase().endsWith(expected.toLowerCase())), JSON.stringify(panel));
    await game.input.trigger("ToggleMap");
    await game.step({ frames: 3 });
  }
  await page("MEDSCI2/english/PAGE001.PCX");
  await acquireOsUpgrade(game, "Spatially Aware");
  await page("MEDSCI2/english/PAGE001A.PCX");
  await game.transitionLevel("medsci1.mis");
  await page("MEDSCI1/english/PAGE001A.PCX");
});
