import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { dot, quatRotate, sub } from "../src/vec.js";

for (const vr of [false, true]) {
  test(`thumbstick movement, turning and opt-in player flight (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [],
    });
    const movement = vr ? "left_hand.thumbstick" : "right_hand.thumbstick";
    const turn = vr ? "right_hand.thumbstick" : "left_hand.thumbstick";
    await game.input.set("head.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 60 });
    const start = (await game.info()).player;
    await game.input.set(movement, [0, 1]);
    await game.step({ frames: 10 });
    await game.input.set(movement, [0, 0]);
    await game.step({ frames: 2 });
    const walked = (await game.info()).player;
    assert.ok(dot(sub(walked.position, start.position), quatRotate(start.rotation, [0, 0, -1])) > 0.5, "movement stick walks forward");
    assert.ok(Math.abs(walked.position[1] - start.position[1]) < 0.05, "forward does not fly");
    assert.deepEqual(walked.rotation, start.rotation, "movement does not turn");

    await game.input.set(movement, [1, 0]);
    await game.step({ frames: 10 });
    await game.input.set(movement, [0, 0]);
    await game.step({ frames: 2 });
    const strafed = (await game.info()).player;
    assert.ok(dot(sub(strafed.position, walked.position), quatRotate(start.rotation, [-1, 0, 0])) > 0.5, "movement stick strafes");
    assert.deepEqual(strafed.rotation, start.rotation, "strafe does not turn");

    for (const vertical of [1, -1]) {
      await game.input.set(turn, [0, vertical]);
      await game.step({ frames: 10 });
      const idle = (await game.info()).player;
      assert.ok(Math.hypot(...idle.position.map((v, i) => v - strafed.position[i])) < 0.05,
        `turn-stick up/down does nothing by default: ${strafed.position} -> ${idle.position}`);
    }
    await game.input.set(turn, [1, 0]);
    await game.step({ frames: 10 });
    await game.input.set(turn, [0, 0]);
    const turned = (await game.info()).player;
    assert.ok(Math.abs(turned.rotation.reduce((dot, v, i) => dot + v * start.rotation[i], 0)) < 0.999,
      "turn stick rotates the player");
    assert.ok(Math.hypot(...turned.position.map((v, i) => v - strafed.position[i])) < 0.05,
      "turning does not translate the player");

    await game.devParams.set("player_flight", 1);
    await game.input.set(turn, [0, 1]);
    await game.step({ frames: 10 });
    const flying = await game.player.position();
    assert.ok(flying.y > turned.position[1] + 0.5, "flight requires explicit opt-in");
    await game.devParams.set("player_flight", 0);
    await game.step({ frames: 10 });
    assert.ok((await game.player.position()).y < flying.y, "disabling flight stops held-stick ascent");
    await game.input.set(turn, [0, 0]);
  });
}
