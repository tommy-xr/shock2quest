import assert from "node:assert/strict";
import { setTimeout } from "node:timers/promises";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// #1921: /info used the HTTP loop's wall clock, even when /step supplied a
// fixed or recorded clock to gameplay. Read-only requests must not advance it.
test("frame snapshots retain the completed simulation clock while idle", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal" });
  const stepped = await game.step({ frames: 60 });
  const first = await game.info();
  assert.equal(first.frame_index, stepped.new_frame_index);
  assert.ok(Math.abs(first.time.total_ms - stepped.new_total_time * 1000) < 1.1,
    `snapshot ${first.time.total_ms}ms must match stepped ${stepped.new_total_time}s`);
  assert.equal(first.time.elapsed_ms, 16, "last completed frame used the fixed 60Hz dt");

  // A short wall-clock delay makes the old reporting fail even when an HTTP
  // request happened to arrive near the simulated timestamp by coincidence.
  await setTimeout(40);
  const idle = await game.info();
  assert.equal(idle.frame_index, first.frame_index);
  assert.deepEqual(idle.time, first.time, "idle HTTP requests keep the last frame's clock");

  const nextStep = await game.step({ frames: 1 });
  const next = await game.info();
  assert.equal(next.frame_index, first.frame_index + 1);
  assert.ok(Math.abs(next.time.total_ms - nextStep.new_total_time * 1000) < 1.1);
  assert.equal(next.time.elapsed_ms, 16);
  assert.ok(next.time.total_ms > first.time.total_ms);
});
