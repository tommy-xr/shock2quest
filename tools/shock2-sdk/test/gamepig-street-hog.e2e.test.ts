import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`Street Hog traffic, held hops, lives and campaign scores (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    const { click } = await openGamePig(game, vr, "Hogger Cart");
    const elements = async () => (await game.ui.state()).active_panel!.elements;
    const control = async (label: string) => {
      const found = (await elements()).find(e => e.label === label);
      assert.ok(found, `missing playable ${label}`);
      return found;
    };
    const field = async () => (await elements()).find(e => e.texture === "hogback.pcx")!.rect;
    const pig = async () => {
      const y = (await field())[1] + 256;
      return (await elements()).find(e => /^hog2?\.pcx$/.test(e.texture ?? "") && e.rect[1] < y - 1)!;
    };
    const lives = async () => {
      const y = (await field())[1] + 256;
      return (await elements()).filter(e => /^hog2?\.pcx$/.test(e.texture ?? "") && e.rect[1] >= y - 1).length;
    };
    await click(game, await control("game-7"));
    assert.equal(await lives(), 2);
    const initial = (await pig()).rect;
    if (vr) {
      await game.input.set("left_hand.thumbstick", [0, 1]);
      await game.step({ frames: 3 });
      await game.input.set("left_hand.thumbstick", [0, 0]);
      await game.step({ frames: 3 });
    } else await click(game, await control("hog-0-0"));
    assert.equal(Math.round((await pig()).rect[1] - initial[1]), -16, "one directional gesture hops one row");
    const traffic = async () => (await elements()).filter(e => /^hog(tr|ra|do)/.test(e.texture ?? "")).map(e => e.rect);
    const before = await traffic();
    await game.step({ frames: 30 });
    assert.notDeepEqual(await traffic(), before, "traffic advances with simulation time");
    if (vr) {
      assert.equal(Math.round((await pig()).rect[1] - initial[1]), -16, "neutral stick does not repeat hops");
      const rotation = (await game.info()).player.rotation;
      await game.input.set("right_hand.thumbstick", [1, 0]);
      await game.step({ frames: 12 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.notDeepEqual((await game.info()).player.rotation, rotation, "free hand still turns");
    }
    for (let death = 0; death < 3; death++) {
      const row = Math.round(((await pig()).rect[1] - (await field())[1]) / 16);
      const target = await control(`hog-1-${row}`);
      await click(game, target);
      const entered = (await elements()).find(e => /^(hog2?|hogsp2?)\.pcx$/.test(e.texture ?? "") && e.rect[1] < target.rect[1] + 1)!;
      assert.equal(Math.round(entered.rect[0]), Math.round(target.rect[0]), "hop enters first traffic lane");
      await game.step({ frames: 900 });
      if (death < 2) assert.equal(await lives(), 1 - death);
    }
    await control("hog-restart");
    const name = `street_hog_${vr}_${Date.now()}`;
    try {
      await game.save(name);
      const scores = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_game_scores["7"];
      assert.equal(scores().length, 1, "final death records once");
      const stored = scores();
      await game.step({ frames: 120 });
      await click(game, await control("hog-restart"));
      assert.equal(await lives(), 2);
      await game.save(name);
      assert.deepEqual(scores(), stored);
      await game.load(name);
      await game.step({ frames: 3 });
      await game.save(name);
      assert.deepEqual(scores(), stored, "same-build saves retain scores");
    } finally { rmSync(savedGamePath(name), { force: true }); }
    const device = (await game.entities.list({ filter: "Gameboy" })).entities[0]!;
    await game.entities.sendMessage(device.id, { type: "SetObjectState", state: "Hacked" });
    await game.entities.sendMessage(device.id, { type: "Frob" });
    await game.step({ frames: 3 });
    await click(game, await control("game-7"));
    assert.equal(await lives(), 3, "hacked device starts with an extra spare life");
  });
}
