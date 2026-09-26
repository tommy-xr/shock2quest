import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("a recorded session replays from its start save", { skip: !enabled, timeout: 300_000 }, async () => {
  let path: string;
  let recordedEnd: number[];
  {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    await game.input.trigger("ToggleInputRecording");
    await game.step({ frames: 1 });
    // Walk and turn: position depends on every recorded frame.
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.input.set("left_hand.thumbstick", [0.5, 0]);
    await game.step({ frames: 45 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.input.trigger("ToggleInputRecording");
    await game.step({ frames: 1 });
    recordedEnd = (await game.info()).player.position;
    const saved = (await game.ui.state()).messages.find((m) => m.startsWith("Recording saved: "));
    assert.ok(saved, "stopping reports the recording's path");
    path = saved.slice("Recording saved: ".length);
  }
  try {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
    await game.step({ frames: 10 });
    const start = (await game.info()).player.position;
    const { frames, scene } = await game.replay(path);
    assert.equal(scene, "medsci1.mis");
    await game.step({ frames });
    const end = (await game.info()).player.position;
    const moved = Math.hypot(end[0] - start[0], end[2] - start[2]);
    const error = Math.hypot(end[0] - recordedEnd[0], end[2] - recordedEnd[2]);
    assert.ok(moved > 1, `the replay walks (moved ${moved})`);
    assert.ok(error < 0.1, `replay ends where the session did (off by ${error})`);
  } finally {
    await rm(path, { force: true });
    await rm(path.replace(/\.jsonl$/, ".sav"), { force: true });
  }
});
