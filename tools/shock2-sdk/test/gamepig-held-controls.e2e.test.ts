import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const hand of ["left", "right"] as const) {
  test(`held GamePig captures only its ${hand} controller`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
      await game.step({ frames: 60 });
      await game.input.set(`${hand}_hand.squeeze`, 1);
      const handheld = await game.player.spawnItem("Gameboy", { hand });
      await game.step({ frames: 3 });
      const cart = await game.player.spawnItem("Swinekeeper");
      await game.entities.sendMessage(cart.entity_id, { type: "Frob" });
      await game.step({ frames: 3 });
      const lower = hand === "left" ? "LeftHandLowerButton" : "RightHandLowerButton";
      const upper = hand === "left" ? "LeftHandUpperButton" : "RightHandUpperButton";
      assert.equal((await game.ui.state()).active_panel, null, "held device starts closed");
      if (hand === "left") await game.input.trigger(lower);
      else await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 80 }); // Startup consumes the opening gesture.
      assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-3"),
        "first primary or navigation opens the index and is consumed");
      await game.input.set(`${hand}_hand.thumbstick`, [0, 0]);
      await game.step({ frames: 2 });
      assert.equal((await game.ui.state()).active_panel!.entity_id, handheld.entity_id);
      assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.kind === "fill"),
        "controller opening immediately shows the focused index row");
      const resting = await game.player.position();
      const restingRotation = (await game.info()).player.rotation;
      await game.input.set(`${hand}_hand.thumbstick`, [1, 0]);
      await game.step({ frames: 12 });
      await game.input.set(`${hand}_hand.thumbstick`, [0, 0]);
      await game.step({ frames: 2 });
      const stationary = await game.player.position();
      assert.deepEqual((await game.info()).player.rotation, restingRotation, "captured stick does not turn the player");
      assert.ok(Math.hypot(stationary.x-resting.x, stationary.y-resting.y, stationary.z-resting.z) < 0.02,
        "captured stick does not move the player");
      await game.input.trigger(lower);
      await game.step({ frames: 3 });
      assert.ok(Math.abs((await game.player.position()).y-resting.y) < 0.02, "primary does not jump");
      const pose = (await game.ui.state()).panel_pose;
      assert.ok(pose && pose.size[0] < 0.23 && pose.size[1] < 0.34, "UI is mounted on the physical screen aperture");
      let elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(elements.some(e => e.texture === "swinebck.pcx"), "primary selects the highlighted installed game instead of jumping");
      await game.input.set(`${hand}_hand.thumbstick`, [1, 0]);
      await game.step({ frames: 3 });
      await game.input.set(`${hand}_hand.thumbstick`, [0, 0]);
      await game.step({ frames: 2 });
      await game.input.trigger(upper);
      await game.step({ frames: 3 });
      await game.input.trigger(lower);
      await game.step({ frames: 3 });
      elements = (await game.ui.state()).active_panel!.elements;
      assert.ok(elements.some(e => e.text === "11"), "secondary arms flag placement, primary flags the focused tile");
      assert.ok(elements.some(e => e.texture === "swineflg.pcx" && e.rect[0] === 37), "stick moves the focused cell right");
      await game.input.hold(upper);
      await game.step({ frames: 36 });
      await game.input.release(upper);
      await game.step({ frames: 3 });
      assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.label === "game-3"), "holding secondary returns to the game list");
      const otherLower = hand === "left" ? "RightHandLowerButton" : "LeftHandLowerButton";
      const beforeJump = (await game.player.position()).y;
      await game.input.trigger(otherLower);
      let peak = beforeJump;
      for (let frame = 0; frame < 30; frame++) {
        await game.step({ frames: 1 });
        peak = Math.max(peak, (await game.player.position()).y);
      }
      assert.ok(peak > beforeJump + 0.2, "other hand still jumps normally");
      await game.step({ frames: 60 });
      const beforeOtherStick = (await game.info()).player;
      const otherHand = hand === "left" ? "right" : "left";
      await game.input.set(`${otherHand}_hand.thumbstick`, [1, 0]);
      await game.step({ frames: 12 });
      await game.input.set(`${otherHand}_hand.thumbstick`, [0, 0]);
      await game.step({ frames: 2 });
      const afterOtherStick = (await game.info()).player;
      if (otherHand === "right") assert.notDeepEqual(afterOtherStick.rotation, beforeOtherStick.rotation,
        "other hand can still turn normally");
      else assert.ok(Math.hypot(...afterOtherStick.position.map((v, i) => v - beforeOtherStick.position[i]!)) > 0.05,
        "other hand can still move normally");
      // Releasing the device with controls held cannot turn them into a jump
      // or movement. Normal controls return only after release and neutral.
      await game.input.hold(lower);
      await game.input.set(`${hand}_hand.thumbstick`, [1, 0]);
      await game.step({ frames: 3 });
      const beforeDrop = await game.player.position();
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 15 });
      const afterDrop = await game.player.position();
      assert.ok(Math.hypot(afterDrop.x-beforeDrop.x, afterDrop.y-beforeDrop.y, afterDrop.z-beforeDrop.z) < 0.02,
        "dropping with held controls does not leak a jump or movement");
      await game.input.release(lower);
      await game.input.set(`${hand}_hand.thumbstick`, [0, 0]);
      await game.step({ frames: 3 });
      await game.input.trigger(lower);
      let resumedPeak = afterDrop.y;
      for (let frame = 0; frame < 30; frame++) {
        await game.step({ frames: 1 });
        resumedPeak = Math.max(resumedPeak, (await game.player.position()).y);
      }
      assert.ok(resumedPeak > afterDrop.y + 0.2, "released hand resumes normal jump on a fresh press");
    });
}
