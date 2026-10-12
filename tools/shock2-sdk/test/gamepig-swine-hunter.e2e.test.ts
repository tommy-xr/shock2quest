import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`Swine Hunter steers, fires, recovers and retains scores (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    const { handheld, click } = await openGamePig(game, vr, "Swine Hunter Cart");
    const elements = async () => (await game.ui.state()).active_panel!.elements;
    const control = async (label: string) => {
      const element = (await elements()).find(e => e.label === label);
      assert.ok(element, `missing playable ${label}`);
      return element;
    };
    const car = async () => {
      const element = (await elements()).find(e => e.texture === "racecar.pcx");
      assert.ok(element, "player car must be active");
      return element;
    };
    const point = async (x: number, y: number) => {
      const field = await control("hunter-play");
      const [left, top, width, height] = field.screen_rect;
      await game.input.set("pointer.position", [left + x / 140 * width, top + y / 276 * height]);
    };
    const secondary = async () => {
      if (vr) {
        await game.input.hold("LeftHandUpperButton");
        await game.step({ frames: 2 });
        await game.input.release("LeftHandUpperButton");
      } else {
        const player = await car();
        const field = await control("hunter-play");
        await point(player.rect[0] + 6 - field.rect[0], player.rect[1] + 6 - field.rect[1]);
        await game.input.set("pointer.secondary", 1);
        await game.step({ frames: 2 });
        await game.input.set("pointer.secondary", 0);
      }
      await game.step({ frames: 2 });
    };
    await click(game, await control("game-9"));
    assert.ok((await elements()).some(e => e.texture === "raceback.pcx"));
    assert.equal((await elements()).filter(e => e.texture === "racewall.pcx").length, 180);
    const start = (await car()).rect;
    if (vr) await game.input.set("left_hand.thumbstick", [-1, 1]);
    else await point(5, start[1] - (await control("hunter-play")).rect[1] - 60);
    await game.step({ frames: 12 });
    const moved = (await car()).rect;
    assert.ok(moved[0] <= start[0] - 6 && moved[1] <= start[1] - 6, `owning input steers both axes: ${JSON.stringify({ start, moved })}`);
    if (vr) {
      await game.input.set("left_hand.thumbstick", [0, 0]);
      await game.step({ frames: 6 });
      assert.deepEqual((await car()).rect, moved, "neutral input stops steering");
      const rotation = (await game.info()).player.rotation;
      await game.input.set("right_hand.thumbstick", [1, 0]);
      await game.step({ frames: 6 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.notDeepEqual((await game.info()).player.rotation, rotation, "other hand keeps turning");
      assert.deepEqual((await car()).rect, moved, "other hand cannot steer the game");
    }
    await secondary();
    assert.equal((await elements()).filter(e => e.texture === "raceshot.pcx").length, 0, "ordinary GamePig has no rear shot");
    await game.entities.sendMessage(handheld.entity_id, { type: "SetObjectState", state: "Hacked" });
    await game.step({ frames: 2 });
    await secondary();
    let shots = (await elements()).filter(e => e.texture === "raceshot.pcx");
    assert.equal(shots.length, 1);
    assert.ok(shots[0]!.rect[1] > (await car()).rect[1], "hacked shot travels behind the car");
    if (vr) {
      await game.input.hold("LeftHandLowerButton");
      await game.step({ frames: 2 });
      await game.input.release("LeftHandLowerButton");
      await game.step({ frames: 2 });
    } else await click(game, await car());
    shots = (await elements()).filter(e => e.texture === "raceshot.pcx");
    assert.equal(shots.length, 2, "forward and hacked rear shots can coexist");
    const playerY = (await car()).rect[1];
    assert.ok(shots.some(shot => shot.rect[1] < playerY));
    let crashed = false;
    let recovered = false;
    if (vr) await game.input.set("left_hand.thumbstick", [-1, 0]);
    else await point(0, playerY - (await control("hunter-play")).rect[1]);
    for (let step = 0; step < 100; step++) {
      await game.step({ frames: 30 });
      const drawn = await elements();
      if (drawn.some(e => e.label === "hunter-restart")) break;
      const lives = drawn.find(e => e.text?.startsWith("Lives:"))?.text;
      crashed ||= drawn.some(e => e.texture === "racecrsh.pcx");
      recovered ||= crashed && lives !== "Lives: 3" && drawn.some(e => e.texture === "racecar.pcx");
    }
    if (vr) await game.input.set("left_hand.thumbstick", [0, 0]);
    assert.ok(crashed && recovered, "wall impacts show wrecks and recover before the final loss");
    await control("hunter-restart");
    const text = (await elements()).find(e => /^\d+ Points$/.test(e.text ?? ""))?.text;
    assert.ok(text, "game over displays the campaign score");
    await game.step({ frames: 60 });
    const name = `hunter_${vr}_${Date.now()}`;
    try {
      const stored = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_game_scores["9"];
      await game.save(name);
      const scores = stored();
      assert.equal(scores.length, 1, "only the final crash records a score");
      assert.equal(scores[0].text, text);
      await click(game, await control("hunter-restart"));
      assert.ok((await elements()).some(e => e.text === "Score: 0"));
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      await game.save(name);
      assert.deepEqual(stored(), scores, "restart and reopen retain scores");
      await game.load(name);
      await game.step({ frames: 2 });
      await game.save(name);
      assert.deepEqual(stored(), scores, "same-build save/load retains scores");
    } finally { rmSync(savedGamePath(name), { force: true }); }
  });
}
