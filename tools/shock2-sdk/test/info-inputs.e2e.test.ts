import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Negative-first: /v1/control/input accepted and used these controls while
// /v1/info.inputs returned only hard-coded defaults.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "/v1/info reports the live controls used for the current frame",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal",
    });

    for (const [channel, value] of [
      ["head.rotation", [0, 1, 0, 0]],
      ["left_hand.position", [1.25, -2.5, 3.75]],
      ["left_hand.rotation", [1, 0, 0, 0]],
      ["left_hand.thumbstick", [0.25, -0.75]],
      ["left_hand.trigger", 0.2],
      ["left_hand.squeeze", 0.4],
      ["left_hand.a", 0.6],
      ["right_hand.position", [-1.5, 2.25, -3]],
      ["right_hand.rotation", [0, 0, 1, 0]],
      ["right_hand.thumbstick", [-0.5, 0.75]],
      ["right_hand.trigger", 0.3],
      ["right_hand.squeeze", 0.5],
      ["right_hand.a", 0.7],
    ] as const) {
      await game.input.set(channel, value);
    }

    await game.step({ frames: 1 });

    const info = (await game.info()) as {
      inputs: {
        head_rotation: number[];
        hands: {
          left: {
            position: number[];
            rotation: number[];
            thumbstick: number[];
            trigger: number;
            squeeze: number;
            a: number;
          };
          right: {
            position: number[];
            rotation: number[];
            thumbstick: number[];
            trigger: number;
            squeeze: number;
            a: number;
          };
        };
      };
    };

    assert.deepEqual(info.inputs, {
      head_rotation: [0, 1, 0, 0],
      hands: {
        left: {
          position: [1.25, -2.5, 3.75],
          rotation: [1, 0, 0, 0],
          thumbstick: [0.25, -0.75],
          trigger: 0.2,
          squeeze: 0.4,
          a: 0.6,
        },
        right: {
          position: [-1.5, 2.25, -3],
          rotation: [0, 0, 1, 0],
          thumbstick: [-0.5, 0.75],
          trigger: 0.3,
          squeeze: 0.5,
          a: 0.7,
        },
      },
    });
  },
);
