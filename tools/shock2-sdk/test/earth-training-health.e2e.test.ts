import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };

test("Earth basic training entry permits the hypo lesson", options, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 5 });
  // Walk through the real entry sensor. No damage or script message injection.
  const [entry] = await game.entities.byTemplate(448);
  assert.ok(entry);
  await game.player.moveTo({ x: entry.position[0], y: entry.position[1], z: entry.position[2] });
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.hit_points, 15, "ReduceHP must run through the authored entry link");

  // Provision the authored hypo into the backpack; use it through the
  // production inventory controls, with no health cheats.
  const [hypo] = await game.entities.byTemplate(487);
  assert.ok(hypo);
  await game.player.give(hypo.id);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const strip = (await game.ui.state()).strip!.elements;
  const hypoButton = strip.find(e => e.kind === "button" && e.entity_id === hypo.id)!;
  assert.ok(hypoButton);
  await clickUiElement(game, hypoButton);
  await clickUiElement(game, hypoButton);
  await game.step({ frames: 400 });
  assert.ok((await game.info()).player.hit_points! > 15, "the training hypo should now heal");
});

