import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { launchWithSettings } from "./helpers/settings.js";
import { quatConjugate, quatRotate } from "./helpers/vr-hand.js";

const skip = process.env.SHOCK2_E2E !== "1" && "set SHOCK2_E2E=1 to run";
const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 0.01, `${a} != ${b}`);

const ON = { vr: { roomscale: "On" } };

test("roomscale moves the body once, blocks at a wall and retreats without stored travel", { skip }, async (t) => {
  await using game = await launchWithSettings(t, ON, { mission: "debug_ladder", debugFlags: ["--vr"] });
  await game.player.teleport({ x: -5, y: 1, z: -16 });
  await game.input.set("head.position", [0, 1.04, 0]);
  await game.step({ frames: 30 });
  const initial = (await game.info()).player;
  const inverse = quatConjugate(initial.rotation);
  const sample = async (distance: number) => {
    const local = quatRotate(inverse, [-distance, 0, 0]);
    await game.input.set("head.position", [local[0], 1.04, local[2]]);
    await game.input.set("left_hand.position", [local[0] - 0.3, 0.8, local[2] - 0.3]);
    await game.input.set("right_hand.position", [local[0] + 0.3, 0.8, local[2] - 0.3]);
    await game.step({ frames: 3 });
    return (await game.info()).player;
  };
  const free = await sample(0.2);
  close(free.position[0], initial.position[0] - 0.2);
  close(free.camera_offset[0], 0);
  close(free.camera_offset[2], 0);
  close(free.camera_offset[1], 1.04);
  // The plain wall has its near face at world X=-7. Small increments remain
  // physical movement rather than the discontinuity fallback.
  let blocked = free;
  for (let i = 2; i <= 16; i++) blocked = await sample(i * 0.2);
  assert.ok(blocked.position[0] > -7 && blocked.position[0] < -6,
    `capsule should stop before the wall: ${blocked.position}`);
  const held = await sample(3.2);
  close(held.position[0], blocked.position[0]);
  const retreat = await sample(3.1);
  close(retreat.position[0], blocked.position[0] + 0.1);
  await game.step({ frames: 30 });
  close((await game.info()).player.position[0], retreat.position[0]);
});

test("roomscale adds to stick movement and turning stays centered off-origin", { skip }, async (t) => {
  await using game = await launchWithSettings(t, ON, { mission: "debug_ladder", debugFlags: ["--vr"] });
  await game.player.teleport({ x: 0, y: 1, z: -16 });
  // Spawn while physically away from the tracking origin.
  await game.input.set("head.position", [2, 1.04, 1]);
  await game.step({ frames: 30 });
  const start = (await game.info()).player;
  await game.input.set("right_hand.thumbstick", [1, 0]);
  await game.step({ frames: 15 });
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.step({ frames: 2 });
  const turned = (await game.info()).player;
  close(turned.position[0], start.position[0]);
  close(turned.position[2], start.position[2]);
  close(turned.camera_offset[0], 0);
  close(turned.camera_offset[2], 0);
  assert.notDeepEqual(turned.rotation, start.rotation);

  const stickRun = async (physical: boolean) => {
    const before = (await game.info()).player;
    await game.input.set("left_hand.thumbstick", [0, 0.4]);
    for (let i = 1; i <= 6; i++) {
      if (physical) await game.input.set("head.position", [2 + i * 0.03, 1.04, 1]);
      await game.step({ frames: 1 });
    }
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.step({ frames: 2 });
    const after = (await game.info()).player;
    return after.position.map((v, i) => v - before.position[i]);
  };
  const stick = await stickRun(false);
  assert.ok(Math.hypot(stick[0], stick[2]) > 0.05, `stick run moved: ${stick}`);
  const combined = await stickRun(true);
  const physical = quatRotate(turned.rotation, [0.18, 0, 0]);
  close(combined[0] - stick[0], physical[0]);
  close(combined[2] - stick[2], physical[2]);
});

test("roomscale walking never activates the comfort vignette", { skip }, async (t) => {
  await using game = await launchWithSettings(t, { vr: { vignette: "High", roomscale: "On" } },
    { mission: "debug_ladder", debugFlags: ["--vr"] });
  await game.player.teleport({ x: -5.8, y: 1, z: -16 });
  // Settle the teleport drop (its fall counts as travel) before measuring.
  await game.step({ frames: 90 });
  const mask = () => game.scene.fromSource("vr_comfort_vignette");
  assert.equal((await mask()).length, 0, "vignette idle before walking");
  // Walk physically at ~1 m/s into the wall at X=-6.48 and keep pushing.
  for (let i = 1; i <= 60; i++) {
    await game.input.set("head.position", [-i * 0.017, 1.04, 0]);
    await game.step({ frames: 1 });
    assert.equal((await mask()).length, 0, `physical walking must not vignette (frame ${i})`);
  }
  await game.input.set("left_hand.thumbstick", [0, 1]);
  await game.step({ frames: 20 });
  assert.ok((await mask()).length > 0, "stick travel still vignettes");
});

// Moves the head 0.2 sideways and reports whether the body followed.
async function walks(game: GameServer, bounds: [number, number] | null, vr = true) {
  await game.input.set("roomscale.bounds", bounds);
  await game.step({ frames: 3 });
  const before = (await game.info()).player;
  const x = before.camera_offset[0] + 0.2;
  await game.input.set("head.position", [x, 1.04, 0]);
  await game.step({ frames: 3 });
  const after = (await game.info()).player;
  const moved = Math.hypot(after.position[0] - before.position[0], after.position[2] - before.position[2]);
  // A still body leaves the step in the rig; a walking one recentres it.
  close(after.camera_offset[0], moved > 0.01 || !vr ? 0 : x);
  return moved > 0.01;
}

test("Auto roomscale needs a room-sized guardian", { skip }, async () => {
  await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
  await game.player.teleport({ x: 0, y: 1, z: -16 });
  await game.step({ frames: 30 });
  assert.equal(await walks(game, null), false, "no bounds is stationary");
  assert.equal(await walks(game, [1, 1]), false, "a cramped guardian is stationary");
  assert.equal(await walks(game, [2, 2]), true, "a room-sized guardian walks");
});

test("Off roomscale ignores the guardian; flat ignores On", { skip }, async (t) => {
  {
    await using game = await launchWithSettings(t, { vr: { roomscale: "Off" } },
      { mission: "debug_ladder", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    assert.equal(await walks(game, [2, 2]), false);
  }
  {
    await using game = await launchWithSettings(t, ON, { mission: "debug_ladder" });
    await game.step({ frames: 30 });
    assert.equal(await walks(game, [2, 2], false), false);
  }
});
