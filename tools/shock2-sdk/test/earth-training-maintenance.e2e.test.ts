import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };

test("Earth maintenance allowance survives reload without upgrading the career", options, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 5 });
  const [tool] = await game.entities.byTemplate(264);
  const [gun] = await game.entities.byTemplate(246);
  assert.ok(tool && gun);
  for (const item of [tool, gun]) await game.player.give(item.id);
  await game.save("earth-training-allowance");
  await game.load("earth-training-allowance");
  await game.step({ frames: 5 });
  if ((await game.ui.state()).mode !== "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
  }
  const [restoredTool] = await game.entities.byTemplate(264);
  const [restoredGun] = await game.entities.byTemplate(246);
  const elements = (await game.ui.state()).strip!.elements;
  const source = elements.find(e => e.kind === "button" && e.entity_id === restoredTool.id)!;
  const target = elements.find(e => e.kind === "button" && e.entity_id === restoredGun.id)!;
  assert.ok(source && target);
  await clickUiElement(game, source);
  await clickUiElement(game, target);
  await game.step({ frames: 5 });
  const condition = (await game.entities.detail(restoredGun.id)).properties.find(p => p.name === "Condition");
  assert.equal(Number(condition?.value), 100, "Earth's Maintenance 5 restores the authored 50-condition pistol");
  assert.equal((await game.entities.byTemplate(264)).length, 0, "one successful maintenance consumes the tool");
  assert.equal((await game.info()).player.stats?.skills.maintenance, 0, "training must not upgrade the career sheet");
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 5 });
  assert.equal((await game.info()).player.stats?.skills.maintenance, 0);
});
