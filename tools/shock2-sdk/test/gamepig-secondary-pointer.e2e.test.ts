import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";

test("Swinekeeper secondary pointer flags once, rearms, and returns from a finished game", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  const { click, handheld } = await openGamePig(game, false, "Swinekeeper");
  const elements = async () => (await game.ui.state()).active_panel!.elements;
  const control = async (label: string) => {
    const element = (await elements()).find(e => e.label === label);
    assert.ok(element, `missing ${label}`);
    return element;
  };
  const point = async (label: string) => {
    const [x, y, w, h] = (await control(label)).screen_rect;
    await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
  };
  const secondary = async (value: number) => {
    await game.input.set("pointer.secondary", value);
    await game.step({ frames: 3 });
  };
  const flagCount = async () => (await elements()).filter(e => e.texture === "swineflg.pcx").length;
  await click(game, await control("game-3"));
  await point("swine-cell-0");
  await secondary(0);
  await secondary(1);
  assert.equal(await flagCount(), 1);
  await game.step({ frames: 12 });
  assert.equal(await flagCount(), 1, "held secondary emits one gesture");
  await secondary(0);
  await click(game, await control("swine-cell-0"));
  assert.equal(await flagCount(), 1, "primary cannot reveal a flagged tile");
  await secondary(1);
  assert.equal(await flagCount(), 0, "fresh secondary removes the flag");
  await secondary(0);
  await game.input.set("pointer.position", [0.9, 0.9]);
  await secondary(1);
  await point("swine-cell-0");
  await game.step({ frames: 3 });
  assert.equal(await flagCount(), 0, "press outside cannot drag a held secondary into a tile");
  await secondary(0);
  await game.input.set("pointer.position", null);
  await game.step({ frames: 3 });
  await game.input.set("pointer.secondary", 1);
  await point("swine-cell-0");
  await game.step({ frames: 3 });
  assert.equal(await flagCount(), 0, "pointer recovery requires release");
  await secondary(0);
  await secondary(1);
  assert.equal(await flagCount(), 1);
  await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
  await game.step({ frames: 3 });
  await click(game, await control("game-3"));
  await point("swine-cell-0");
  await game.step({ frames: 3 });
  assert.equal(await flagCount(), 0, "opening while held does not queue a flag");
  await secondary(0);
  await secondary(1);
  assert.equal(await flagCount(), 1);
  await secondary(0);
  await secondary(1);
  assert.equal(await flagCount(), 0);
  await secondary(0);
  for (let i = 0; i < 60; i++) {
    if ((await elements()).some(e => e.label === "swine-restart")) break;
    await click(game, await control(`swine-cell-${i}`));
  }
  assert.ok((await elements()).some(e => e.label === "swine-restart"));
  await secondary(1);
  assert.ok((await elements()).some(e => e.label === "game-3"), "secondary returns from a completed game to the index");
});
