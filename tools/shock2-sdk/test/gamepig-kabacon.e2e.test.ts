import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { savedGamePath } from "./helpers/save.js";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";

for (const vr of [false, true]) {
  test(`Hurling Bacon catches falling bacon with pointer or held thumbstick (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      const { handheld, click } = await openGamePig(game, vr, "KaBacon Cart");
      let elements = (await game.ui.state()).active_panel!.elements;
      const select = elements.find(e => e.label === "game-5");
      assert.ok(select, "installed Hurling Bacon must be selectable");
      await click(game, select);
      elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(elements.some(e => e.texture === "kbbomber.pcx"));
      assert.equal(elements.filter(e => e.texture?.startsWith("kbpad_")).length, 3);
      const paddle = () => elements.find(e => e.texture?.startsWith("kbpad_"))!;
      const moveTo = async (target: number) => {
        if (vr) {
          const x = paddle().rect[0] + 12;
          await game.input.set("left_hand.thumbstick", [Math.max(-1, Math.min(1, (target - x) / 24)), 0]);
        } else {
          const field = elements.find(e => e.kind === "fill")!;
          const [x, y, w, h] = field.screen_rect;
          await click(game, { ...field, screen_rect: [x + (target - 13) / 140 * w, y + h * 0.7, 0, 0] });
        }
      };
      const initial = paddle().rect[0];
      await moveTo(25);
      await game.step({ frames: 20 });
      elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(paddle().rect[0] < initial - 20, "continuous input moves the paddle left");
      if (vr) {
        await game.input.set("left_hand.thumbstick", [0, 0]);
        const stopped = paddle().rect[0];
        await game.step({ frames: 12 });
        elements = (await game.ui.state()).active_panel!.elements;
        assert.equal(paddle().rect[0], stopped, "neutral input stops immediately");
      }
      let caught = 0;
      for (let frame = 0; frame < 300 && caught < 2; frame++) {
        const falling = elements.filter(e => e.texture === "kbbacon.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
        if (falling) await moveTo(falling.rect[0] + 6);
        await game.step({ frames: 6 });
        elements = (await game.ui.state()).active_panel!.elements;
        caught = Number(elements.find(e => e.text?.startsWith("Bacon Caught:"))?.text?.split(":")[1]);
      }
      assert.ok(caught >= 2, "paddle catches falling bacon and increments the score");
      let restored = false;
      for (let frame = 0; frame < 600 && !elements.some(e => e.label === "bacon-restart"); frame++) {
        const paddles = elements.filter(e => e.texture?.startsWith("kbpad_"));
        if (paddles.length > 0) {
          const falling = elements.filter(e => e.texture === "kbbacon.pcx").sort((a, b) => b.rect[1] - a.rect[1])[0];
          await moveTo(falling && falling.rect[0] < 80 ? 140 : 25);
          if (paddles.length === 2 && !restored) {
            await click(game, elements.find(e => e.label === "bacon-bomber")!);
            elements = (await game.ui.state()).active_panel!.elements;
            assert.equal(elements.filter(e => e.texture?.startsWith("kbpad_")).length, 2, "ordinary device cannot restore a paddle");
            await game.entities.sendMessage(handheld.entity_id, { type: "SetObjectState", state: "Hacked" });
            await game.step({ frames: 2 });
            elements = (await game.ui.state()).active_panel!.elements;
            await click(game, elements.find(e => e.label === "bacon-bomber")!);
            elements = (await game.ui.state()).active_panel!.elements;
            assert.equal(elements.filter(e => e.texture?.startsWith("kbpad_")).length, 3, "hacked bomber activation restores one paddle");
            restored = true;
          }
        }
        await game.step({ frames: 12 });
        elements = (await game.ui.state()).active_panel!.elements;
      }
      if (vr) await game.input.set("left_hand.thumbstick", [0, 0]);
      assert.ok(restored, "lost-paddle reset is exercised");
      assert.ok(elements.some(e => e.label === "bacon-restart"), "losing all paddles reveals scores and delayed restart");
      const scoreText = elements.find(e => /^\d+ Bacon Caught$/.test(e.text ?? ""))?.text;
      assert.ok(scoreText, "campaign high score is rendered");
      const name = `kabacon_${vr}_${Date.now()}`;
      try {
        await game.save(name);
        const scores = () => JSON.parse(readFileSync(savedGamePath(name), "utf8")).global_data.quest_info.mini_game_scores["5"];
        const stored = scores();
        assert.equal(stored.length, 1, "game over records the score only once");
        assert.equal(stored[0].text, scoreText);
        await click(game, elements.find(e => e.label === "bacon-restart")!);
        elements = (await game.ui.state()).active_panel!.elements;
        assert.equal(elements.filter(e => e.texture?.startsWith("kbpad_")).length, 3);
        await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
        await game.step({ frames: 2 });
        assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-5"));
        await game.save(name);
        assert.deepEqual(scores(), stored, "restart and reopen retain campaign scores");
        await game.load(name);
        await game.step({ frames: 2 });
        await game.save(name);
        assert.deepEqual(scores(), stored, "same-build save/load retains scores");
      } finally { rmSync(savedGamePath(name), { force: true }); }
    });
}
