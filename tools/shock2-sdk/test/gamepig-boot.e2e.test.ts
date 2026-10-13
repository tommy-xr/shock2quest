import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";

for (const vr of [false, true]) {
  test(`GamePig startup descends and chimes once per opening (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 30 });
      if (vr) await game.input.set("left_hand.squeeze", 1);
      const handheld = await game.player.spawnItem("Gameboy", vr ? { hand: "left" } : undefined);
      const cart = await game.player.spawnItem("Swinekeeper");
      await game.entities.sendMessage(cart.entity_id, { type: "Frob" });
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      const title = async () => (await game.ui.state()).active_panel?.elements.find(e => e.text === "GamePig");
      const chimes = async () => (await game.audio.recent({ sample: "ibeep1" })).sounds.filter(s => s.source_entity?.name === "Gameboy");
      const first = await title();
      assert.ok(first, "opening starts the GamePig title animation");
      await game.step({ frames: 12 });
      const falling = await title();
      assert.ok(falling && falling.rect[1] > first.rect[1], "title descends in shared canvas pixels");
      assert.equal((await chimes()).length, 0, "no chime before the title settles");
      await game.input.trigger("TogglePauseMenu");
      await game.step({ frames: 1 });
      assert.equal((await game.info()).paused, true);
      await game.step({ frames: 90 });
      assert.equal((await game.info()).paused, true);
      assert.equal((await chimes()).length, 0);
      await game.input.trigger("TogglePauseMenu");
      await game.step({ frames: 1 });
      assert.equal((await game.info()).paused, false);
      if (vr) {
        const resumed = await title();
        assert.ok(resumed && resumed.rect[1] >= falling.rect[1] && resumed.rect[1] <= falling.rect[1] + 4,
          "pause freezes the visible handheld boot timeline");
      } else {
        // The existing pause menu closes flat use mode and its object panel.
        assert.equal((await game.ui.state()).active_panel, null);
        await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
        await game.step({ frames: 14 });
        assert.ok(await title(), "reopening after pause starts a fresh boot");
      }
      if (vr) {
        await game.input.hold("LeftHandLowerButton");
        await game.input.set("left_hand.thumbstick", [0, -1]);
      } else {
        await clickUiElement(game, (await title())!);
      }
      await game.step({ frames: 40 });
      assert.ok(await title(), "title pauses at its resting position before the index");
      assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.text === "TriOptimum"));
      assert.equal((await chimes()).length, 1, "settling emits exactly one cue");
      await game.step({ frames: 35 });
      let elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(elements.some(e => e.label === "game-3"), "boot input cannot queue a game selection");
      assert.equal(await title(), undefined);
      if (vr) {
        await game.input.release("LeftHandLowerButton");
        await game.input.set("left_hand.thumbstick", [0, 0]);
        await game.step({ frames: 2 });
        await game.input.trigger("LeftHandLowerButton");
        await game.step({ frames: 3 });
      } else await clickUiElement(game, elements.find(e => e.label === "game-3")!);
      elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(elements.some(e => e.texture === "swinebck.pcx"));
      assert.equal((await chimes()).length, 1, "selecting a minigame does not replay startup audio");
      if (vr) {
        await game.input.hold("LeftHandUpperButton");
        await game.step({ frames: 36 });
        await game.input.release("LeftHandUpperButton");
        await game.step({ frames: 2 });
        assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-3"));
        assert.equal((await chimes()).length, 1, "returning to the list does not replay startup");
      }
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      assert.ok(await title(), "reopening starts a fresh boot");
      await game.step({ frames: 90 });
      assert.equal((await chimes()).length, 2);
      // Closing before settle must not leave a delayed audio cue behind.
      await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
      await game.step({ frames: 2 });
      if (vr) {
        await game.input.set("left_hand.squeeze", 0);
        await game.step({ frames: 3 });
        const position = await game.player.position();
        await game.player.teleport({ x: position.x + 12, y: position.y, z: position.z });
      } else {
        await game.input.trigger("ToggleUseMode");
      }
      await game.step({ frames: 90 });
      assert.equal((await game.ui.state()).active_panel, null);
      assert.equal((await chimes()).length, 2, "closed startup never emits its pending chime");
    });
}
