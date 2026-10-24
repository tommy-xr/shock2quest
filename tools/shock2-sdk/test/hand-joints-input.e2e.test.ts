import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";

const enabled = process.env.SHOCK2_E2E === "1";

test(
  "tracked hand joints round-trip through the input channel",
  { skip: !enabled, timeout: 120_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: ["--vr"] });
    const joints: Vec3[] = Array.from({ length: 26 }, (_, i) => [i * 0.01, 1, -0.5]);
    await game.input.set("left_hand.joints", joints);
    await game.step({ frames: 1 });
    let state = await game.input.state();
    assert.deepEqual(state.left_hand.joints?.[25], [0.25, 1, -0.5]);
    assert.equal(state.right_hand.joints, undefined, "the other hand stays untracked");
    await assert.rejects(game.input.set("left_hand.joints", joints.slice(1)), /26/);
    await game.input.set("left_hand.joints", null);
    state = await game.input.state();
    assert.equal(state.left_hand.joints, undefined);
  },
);
