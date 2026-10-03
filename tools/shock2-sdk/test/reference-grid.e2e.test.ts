import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

async function withGrid(run: () => Promise<void>): Promise<void> {
  const root = await mkdtemp(join(tmpdir(), "shock2-grid-test-"));
  const path = join(root, "settings.json");
  const previous = process.env.SHOCK2_SETTINGS_PATH;
  process.env.SHOCK2_SETTINGS_PATH = path;
  await writeFile(path, JSON.stringify({ vr: { reference_grid: "DuringMovement", vignette: "Off" } }));
  try { await run(); }
  finally {
    if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
    else process.env.SHOCK2_SETTINGS_PATH = previous;
    await rm(root, { recursive: true, force: true });
  }
}

for (const vr of [false, true]) {
  test(`Reference grid responds to walking and falling, not physical head motion (${vr ? "VR" : "flat"})`, {
    skip: !enabled, timeout: 180_000,
  }, () => withGrid(async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [] });
    const grid = () => game.scene.fromSource("comfort_reference_grid");
    await game.step({ frames: 120 });
    assert.equal((await grid()).length, 0, "resting physics must not trigger the grid");
    await game.input.set("head.look", [45, 20]);
    await game.input.set("head.position", [0.3, 1.04, -0.2]);
    await game.step({ frames: 60 });
    assert.equal((await grid()).length, 0, "physical tracking alone must not trigger the grid");
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 30 });
    assert.equal((await grid()).length, 1);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 90 });
    assert.equal((await grid()).length, 0, "grid fades out after stopping");
    await game.input.set("left_hand.thumbstick", [1, 0]);
    await game.step({ frames: 12 });
    assert.equal((await grid()).length, 0, "snap origin correction is not sustained travel");
    await game.input.set("left_hand.thumbstick", [0, 0]);
    const p = await game.player.position();
    await game.player.teleport({ x: p.x, y: p.y + 5, z: p.z });
    await game.step({ frames: 10 });
    assert.equal((await grid()).length, 1, "falling activates without controller input");
    await game.step({ frames: 180 });
    assert.equal((await grid()).length, 0, "landing releases the grid");
  }));
}

test("Reference grid activates during an authored lift ride with neutral sticks", {
  skip: !enabled, timeout: 180_000,
}, () => withGrid(async () => {
  await using game = await GameServer.launch({ mission: "command1.mis" });
  const grid = () => game.scene.fromSource("comfort_reference_grid");
  await game.step({ frames: 5 });
  const [lift] = await game.entities.byTemplate(431);
  const [button] = await game.entities.byTemplate(469);
  assert.ok(lift && button);
  await game.player.teleport({ x: lift.position[0], y: lift.position[1] + 1.2, z: lift.position[2] });
  await game.step({ frames: 120 });
  assert.equal((await grid()).length, 0);
  const before = await game.player.position();
  await game.entities.sendMessage(button.id, { type: "Frob" });
  await game.step({ frames: 15 });
  assert.ok((await game.player.position()).y > before.y + 0.05, "lift must actually carry the player");
  assert.equal((await grid()).length, 1, "passive lift motion must activate the grid");
  await game.step({ frames: 240 });
  assert.equal((await grid()).length, 0, "grid fades out at the landing");
}));
