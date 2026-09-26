import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const skip = process.env.SHOCK2_E2E !== "1" && "set SHOCK2_E2E=1 to run";
const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 0.002, `${a} != ${b}`);

test("flat lean moves the eye, preserves the pawn, crouches, and recenters in use mode", { skip }, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 30 });
  await game.input.set("head.look", [20, 0]);
  await game.step({ frames: 1 });
  const neutral = (await game.info()).player;
  await game.input.set("lean", 1);
  await game.step({ frames: 12 });
  const right = (await game.info()).player;
  close(Math.hypot(right.camera_offset[0], right.camera_offset[2]), 0.8);
  right.position.forEach((v, i) => close(v, neutral.position[i]));
  assert.notDeepEqual(right.camera_rotation, neutral.camera_rotation);
  // Change the limit while the key stays held; no reload or release needed.
  await game.devParams.set("flat_lean_distance", 1);
  await game.step({ frames: 1 });
  const shorter = (await game.info()).player;
  close(Math.hypot(shorter.camera_offset[0], shorter.camera_offset[2]), 0.4);
  assert.deepEqual(shorter.camera_rotation, right.camera_rotation);
  await game.devParams.set("flat_lean_distance", 0);
  await game.step({ frames: 1 });
  const disabled = (await game.info()).player;
  close(disabled.camera_offset[0], 0);
  close(disabled.camera_offset[2], 0);
  disabled.camera_rotation.forEach((v, i) => close(v, neutral.camera_rotation[i]));
  await game.devParams.set("flat_lean_distance", 2);
  await game.step({ frames: 12 });
  const restored = (await game.info()).player;
  close(Math.hypot(restored.camera_offset[0], restored.camera_offset[2]), 0.8);
  await game.input.set("lean", -1);
  await game.step({ frames: 24 });
  const left = (await game.info()).player;
  close(left.camera_offset[0], -right.camera_offset[0]);
  close(left.camera_offset[2], -right.camera_offset[2]);
  await game.input.set("crouch", 1);
  await game.step({ frames: 15 });
  const crouched = (await game.info()).player;
  assert.ok(crouched.camera_offset[1] < left.camera_offset[1]);
  close(crouched.camera_offset[0], left.camera_offset[0]);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 12 });
  const use = (await game.info()).player;
  close(use.camera_offset[0], 0);
  close(use.camera_offset[2], 0);
});

test("free camera placement while leaned stays at the requested eye", { skip }, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons" });
  await game.step({ frames: 30 });
  await game.input.set("lean", 1);
  await game.step({ frames: 12 });
  const desired: [number, number, number] = [0, 3, 5];
  const placed = await game.camera.set({ position: desired, lookAt: [0, 1, 0] });
  assert.ok(placed.eye_position);
  placed.eye_position.forEach((v, i) => close(v, desired[i]));
  await game.step({ frames: 30 });
  const after = await game.camera.state();
  assert.ok(after.eye_position);
  after.eye_position.forEach((v, i) => close(v, desired[i]));
});

test("VR ignores the synthetic lean axis", { skip }, async () => {
  await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const neutral = (await game.info()).player;
  await game.input.set("lean", 1);
  await game.step({ frames: 12 });
  const after = (await game.info()).player;
  assert.deepEqual(after.camera_offset, neutral.camera_offset);
  assert.deepEqual(after.camera_rotation, neutral.camera_rotation);
});
