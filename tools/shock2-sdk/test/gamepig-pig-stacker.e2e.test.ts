import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`Pig Stacker moves, rotates, drops and retains scores (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    const { handheld, click } = await openGamePig(game, vr, "Pig Stacker Cart");
    const elements = async () => (await game.ui.state()).active_panel!.elements;
    const control = async (label: string) => {
      const element = (await elements()).find(e => e.label === label);
      assert.ok(element, `missing playable ${label}`);
      return element;
    };
    const blocks = async () => (await elements()).filter(e => e.kind === "fill" && e.rect[2] === 11 && e.rect[3] === 11);
    const left = async () => Math.min(...(await blocks()).map(e => e.rect[0]));
    const shape = async () => {
      const cells = await blocks();
      const x = Math.min(...cells.map(e => e.rect[0]));
      const y = Math.min(...cells.map(e => e.rect[1]));
      // UI coordinates round-trip through the VR viewport as floats; compare canvas pixels.
      return cells.map(e => [Math.round(e.rect[0] - x), Math.round(e.rect[1] - y)])
        .sort((a, b) => a[1]! - b[1]! || a[0]! - b[0]!);
    };
    const score = async () => Number((await elements()).find(e => e.text?.startsWith("Level:"))!.text!.split("Score:")[1]);
    const button = async (action: "LeftHandLowerButton" | "LeftHandUpperButton", frames = 3) => {
      await game.input.hold(action);
      await game.step({ frames });
      await game.input.release(action);
      await game.step({ frames: 3 });
    };
    const primary = async () => vr ? button("LeftHandLowerButton") : click(game, await control("stacker-play"));
    const drop = async () => {
      if (vr) await button("LeftHandUpperButton");
      else {
        const [x, y, w, h] = (await control("stacker-play")).screen_rect;
        await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
        await game.input.set("pointer.secondary", 0);
        await game.step({ frames: 2 });
        await game.input.set("pointer.secondary", 1);
        await game.step({ frames: 2 });
        await game.input.set("pointer.secondary", 0);
        await game.step({ frames: 2 });
      }
    };
    await click(game, await control("game-10"));
    await game.step({ frames: 120 });
    assert.equal((await blocks()).length, 4, "procedural falling piece contains four blocks");
    const initialShape = await shape();
    for (let i = 0; i < 4; i++) await primary();
    assert.deepEqual(await shape(), initialShape, "four rotations return to the original shape");
    const before = await left();
    if (vr) await game.input.set("left_hand.thumbstick", [-1, 0]);
    else {
      const [x, y, w, h] = (await control("stacker-play")).screen_rect;
      await game.input.set("pointer.position", [x + w * 24 / 140, y + h / 2]);
    }
    await game.step({ frames: 45 });
    assert.ok(await left() <= before - 12, "continuous input moves by at least one column");
    if (vr) {
      const released = await left();
      await game.input.set("left_hand.thumbstick", [0, 0]);
      await game.step({ frames: 30 });
      const aligned = await left();
      assert.ok(Math.abs(aligned - released) <= 11, "release only completes current pixel-grid alignment");
      await game.step({ frames: 30 });
      assert.equal(await left(), aligned, "neutral input remains stable after alignment");
      const player = (await game.info()).player.rotation;
      await game.input.set("right_hand.thumbstick", [-1, 0]);
      await game.step({ frames: 12 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.notDeepEqual((await game.info()).player.rotation, player, "other hand still turns the player");
      assert.equal(await left(), aligned, "other hand cannot move the piece");
      await game.input.hold("LeftHandUpperButton");
      await game.step({ frames: 20 });
      assert.equal(await score(), 0, "holding B/Y does not drop on press");
      await game.step({ frames: 16 });
      await game.input.release("LeftHandUpperButton");
      await game.step({ frames: 3 });
      await click(game, await control("game-10"));
    }
    await drop();
    assert.equal(await score(), 1, "secondary hard-drops exactly one piece");
    for (let i = 0; i < 30 && !(await elements()).some(e => e.label === "stacker-restart"); i++) await drop();
    assert.ok((await elements()).some(e => e.label === "stacker-restart"), "stacking to the ceiling ends the game");
    const finalScore = await score();
    assert.ok(finalScore > 1);
    assert.ok((await elements()).some(e => e.text?.includes("pts")), "campaign score table is visible");
    await game.step({ frames: 90 });
    const name = `stacker_${vr}_${Date.now()}`;
    try {
      await game.save(name);
      const scores = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_game_scores["10"];
      const stored = scores();
      assert.equal(stored.length, 1, "top-out records once");
      assert.equal(stored[0].points, finalScore);
      await click(game, await control("stacker-restart"));
      assert.equal(await score(), 0);
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 3 });
      await game.save(name);
      assert.deepEqual(scores(), stored, "restart/reopen preserves campaign scores");
      await game.load(name);
      await game.step({ frames: 3 });
      await game.save(name);
      assert.deepEqual(scores(), stored, "same-build save/load retains scores");
    } finally { rmSync(savedGamePath(name), { force: true }); }
  });
}
