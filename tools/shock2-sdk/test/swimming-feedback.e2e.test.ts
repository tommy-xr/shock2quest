import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) test(`swimming: oxygen, drowning, recovery and retail audio (${vr ? "VR" : "flat"})`, {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_rec_pool", debugFlags: vr ? ["--vr"] : [] });
  await game.player.setStats({ endurance: 1 });
  await game.player.teleport({ x: 16, y: -6.8, z: -231.1 });
  await game.step({ frames: 60 });
  const water = async () => (await game.ui.state()).water!;
  assert.equal((await water()).submerged, true);
  assert.equal((await water()).maximum_seconds, 70);
  assert.ok((await game.audio.recent({ sample: "underwa2", playing: true })).sounds.length > 0,
    "head underwater should start the retail underwater ambience");
  assert.ok((await game.audio.recent({ sample: "dive" })).sounds.length > 0);
  const hp = (await game.info()).player.hit_points;
  await game.step({ frames: 40 * 60 });
  assert.equal((await game.info()).player.hit_points, hp, "no early drowning");
  assert.equal((await game.audio.loops()).loops.filter(s => s.sample.toLowerCase().includes("underwa2")).length, 1,
    "ambience must loop, rather than ending after one sample");
  assert.ok((await water()).remaining_seconds < 30);
  assert.equal((await game.audio.recent({ sample: "linebeep" })).sounds.length, 1);
  await game.step({ frames: 30 * 60 });
  assert.equal((await game.info()).player.hit_points, hp! - 3, "first retail drowning tick");
  assert.ok((await game.audio.recent({ sample: "dmgenlo" })).sounds.length > 0,
    "drowning must use the normal player hurt sound");
  await game.step({ frames: 3 * 60 });
  assert.equal((await game.info()).player.hit_points, hp! - 6, "3 damage every 3 seconds");
  // Dry deck: actual head immersion drives both the audio transition and recovery.
  await game.player.teleport({ x: 11, y: -3.8, z: -231.1 });
  await game.step({ frames: 60 });
  assert.equal((await water()).submerged, false);
  assert.ok(Math.abs((await water()).remaining_seconds - 5) < 0.1);
  assert.ok((await game.audio.recent({ sample: "surfaceh" })).sounds.length > 0);
  assert.equal((await game.audio.recent({ sample: "underwa2", playing: true })).sounds.length, 0);
  assert.equal((await game.audio.loops()).loops.filter(s => s.sample.toLowerCase().includes("underwa2")).length, 0);
  await game.step({ frames: 13 * 60 });
  assert.equal((await water()).remaining_seconds, 70);
  assert.equal((await game.info()).player.hit_points, hp! - 6, "surfacing cancels drowning");
  await game.player.setStats({ endurance: 6 });
  await game.step({ frames: 1 });
  assert.equal((await water()).maximum_seconds, 120);
  assert.equal((await water()).remaining_seconds, 120);
});

test("swimming: surface and submerged movement select different sounds", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_rec_pool", debugFlags: ["--vr"] });
  await game.player.teleport({ x: 16, y: -5.8, z: -231.1 });
  await game.input.set("head.look", [0, 0]);
  await game.step({ frames: 60 });
  let water = (await game.ui.state()).water!;
  assert.equal(water.submerged, false, "head above water must breathe even while the body swims");
  assert.equal(water.remaining_seconds, water.maximum_seconds);
  assert.equal((await game.audio.recent({ sample: "swimtop" })).sounds.length, 0, "idle is not a stroke");
  await game.input.set("right_hand.thumbstick", [0, 0.3]);
  await game.step({ frames: 120 });
  assert.ok((await game.audio.recent({ sample: "swimtop" })).sounds.length > 0);
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.player.teleport({ x: 16, y: -6.8, z: -231.1 });
  await game.step({ frames: 1 });
  await game.input.set("right_hand.thumbstick", [0, 0.3]);
  await game.step({ frames: 120 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.ok((await game.audio.recent({ sample: "stroke" })).sounds.length > 0);
  const strokes = (await game.audio.recent({ sample: "stroke" })).sounds.length;
  await game.step({ frames: 120 });
  assert.equal((await game.audio.recent({ sample: "stroke" })).sounds.length, strokes);
  await game.input.trigger("DebugReloadLevel");
  await game.step({ frames: 2 });
  // Repeated transitions must dispose the old loop before the new scene starts one.
  assert.ok((await game.audio.loops()).loops.filter(s => s.sample.toLowerCase().includes("underwa2")).length <= 1);
});

test("swimming: air survives a real mission save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "rec1.mis", debugFlags: ["--vr"] });
  await game.devParams.set("cheat", 1);
  await game.player.setStats({ endurance: 1 });
  await game.player.teleport({ x: 16, y: -6.8, z: -231.1 });
  await game.step({ frames: 300 });
  const before = (await game.ui.state()).water!;
  assert.ok(before.submerged && before.remaining_seconds < 66);
  const save = `swimming-oxygen-${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  await game.player.teleport({ x: 11, y: -3.8, z: -231.1 });
  await game.step({ frames: 120 });
  assert.equal((await game.ui.state()).water!.remaining_seconds, 70);
  assert.equal((await game.load(save)).success, true);
  await game.step({ frames: 1 });
  const restored = (await game.ui.state()).water!;
  assert.equal(restored.submerged, true);
  assert.ok(Math.abs(restored.remaining_seconds - before.remaining_seconds) < 0.1);
});
