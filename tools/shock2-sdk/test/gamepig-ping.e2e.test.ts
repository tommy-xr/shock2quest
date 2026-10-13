import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";

for (const vr of [false, true]) {
  test(`Super Pig Turbo moves and follows pointer or held thumbstick (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      const { click } = await openGamePig(game, vr, "Ping Cart");
      let elements = (await game.ui.state()).active_panel!.elements;
      const select = elements.find(e => e.label === "game-2");
      assert.ok(select, "installed Super Pig Turbo must be selectable");
      await click(game, select);
      elements = (await game.ui.state()).active_panel!.elements;
      const ball = elements.find(e => e.texture === "pongball.pcx");
      assert.ok(ball, "native ball is visible");
      assert.equal(elements.filter(e => e.texture === "pongpadd.pcx").length, 2);
      await game.step({ frames: 30 });
      elements = (await game.ui.state()).active_panel!.elements;
      assert.notDeepEqual(elements.find(e => e.texture === "pongball.pcx")!.rect, ball.rect, "fixed simulation steps advance the ball");
      const backdrop = elements.find(e => e.texture === "pongback.pcx")!;
      const before = elements.filter(e => e.texture === "pongpadd.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
      const [x, y, w, h] = backdrop.screen_rect;
      const playerBefore = (await game.info()).player;
      if (vr) await game.input.set("left_hand.thumbstick", [-1, 0]);
      else await click(game, { ...backdrop, screen_rect: [x + w * 0.1, y + h * 0.7, 0, 0] });
      await game.step({ frames: 30 });
      elements = (await game.ui.state()).active_panel!.elements;
      const after = elements.filter(e => e.texture === "pongpadd.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
      assert.ok(after.rect[0] < before.rect[0], "player paddle follows the input left");
      if (vr) {
        assert.ok(before.rect[0] - after.rect[0] >= 32, "held stick moves continuously beyond a single tick");
        assert.deepEqual((await game.info()).player.rotation, playerBefore.rotation, "paddle input does not turn the player");
        await game.input.set("left_hand.thumbstick", [0, 0]);
        await game.step({ frames: 4 });
        const stopped = (await game.ui.state()).active_panel!.elements.filter(e => e.texture === "pongpadd.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
        await game.step({ frames: 12 });
        const still = (await game.ui.state()).active_panel!.elements.filter(e => e.texture === "pongpadd.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
        assert.equal(still.rect[0], stopped.rect[0], "releasing the stick stops the paddle");
        const beforeOther = (await game.info()).player.position;
        await game.input.set("right_hand.thumbstick", [1, 0]);
        await game.step({ frames: 12 });
        await game.input.set("right_hand.thumbstick", [0, 0]);
        await game.step({ frames: 2 });
        const afterOther = (await game.info()).player.position;
        assert.ok(Math.hypot(...afterOther.map((v, i) => v - beforeOther[i]!)) > 0.05, "other hand still moves the player");
        const isolated = (await game.ui.state()).active_panel!.elements.filter(e => e.texture === "pongpadd.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
        assert.equal(isolated.rect[0], stopped.rect[0], "other hand cannot move the paddle");
        await game.input.hold("LeftHandUpperButton");
        await game.step({ frames: 36 });
        await game.input.release("LeftHandUpperButton");
        await game.step({ frames: 2 });
        assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-2"), "hold returns to the game list");
      }
    });
}
