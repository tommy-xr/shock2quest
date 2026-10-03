import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickCanvas, pauseEntry, vrClickCanvasPoint } from "./helpers/frontend-menu.js";

const enabled = process.env.SHOCK2_E2E === "1";
const DONE: [number, number] = [320, 436];

for (const vr of [false, true]) {
  test(`Comfort effects follow artificial movement and turning (${vr ? "VR" : "flat"})`, {
    skip: !enabled && "set SHOCK2_E2E=1 to run",
  }, async () => {
    const root = await mkdtemp(join(tmpdir(), "shock2-comfort-test-"));
    const path = join(root, "settings.json");
    const previous = process.env.SHOCK2_SETTINGS_PATH;
    process.env.SHOCK2_SETTINGS_PATH = path;
    await writeFile(path, JSON.stringify({ vr: { vignette: "High" } }));
    try {
      await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      const mask = () => game.scene.fromSource("vr_comfort_vignette");
      assert.equal((await mask()).length, 0);
      await game.input.set("head.look", [45, 0]);
      await game.step({ frames: 10 });
      assert.equal((await mask()).length, 0, "physical head motion must not activate the vignette");
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 20 });
      assert.ok((await mask()).length > 0);
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.step({ frames: 30 });
      assert.equal((await mask()).length, 0);
      {
        const rotation = async () => (await game.info()).player.rotation;
        if (vr) await game.input.set("head.position", [0.5, 0.72, -0.2]);
        await game.step({ frames: 3 });
        const eye = async () => {
          const p = (await game.info()).player;
          const [x, y, z, w] = p.rotation; // player snapshot uses XYZW
          const [hx, hy, hz] = p.camera_offset;
          const tx = 2 * (y * hz - z * hy);
          const ty = 2 * (z * hx - x * hz);
          const tz = 2 * (x * hy - y * hx);
          return [p.position[0] + hx + w * tx + y * tz - z * ty,
                  p.position[2] + hz + w * tz + x * ty - y * tx];
        };
        const eyeBefore = await eye();
        const before = await rotation();
        await game.input.set("left_hand.thumbstick", [1, 0]);
        await game.step({ frames: 1 });
        const turned = await rotation();
        const eyeAfter = await eye();
        assert.ok(Math.hypot(...eyeAfter.map((v, i) => v - eyeBefore[i]!)) < 1e-4,
          `snap must keep offset eye fixed: ${eyeBefore} -> ${eyeAfter}`);
        const dot = before.reduce((sum, value, i) => sum + value * turned[i]!, 0);
        const angle = 2 * Math.acos(Math.min(1, Math.abs(dot))) * 180 / Math.PI;
        assert.ok(Math.abs(angle - 30) < 0.05, `expected 30 degree snap, got ${angle}`);
        await game.step({ frames: 30 });
        assert.deepEqual(await rotation(), turned, "held stick cannot repeat a snap");
        await game.input.set("left_hand.thumbstick", [-1, 0]);
        await game.step({ frames: 2 });
        assert.deepEqual(await rotation(), turned, "reversing without centering cannot snap");
        await game.input.set("left_hand.thumbstick", [0, 0]);
        await game.step({ frames: 2 });
        await game.input.set("left_hand.thumbstick", [-1, 0]);
        await game.step({ frames: 1 });
        const returned = await rotation();
        assert.ok(returned.every((value, i) => Math.abs(value - before[i]!) < 1e-5));
        assert.equal((await mask()).length, 0, "snap turns do not trigger the mask");

      }
    } finally {
      if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
      else process.env.SHOCK2_SETTINGS_PATH = previous;
      await rm(root, { recursive: true, force: true });
    }
  });

  test(`Smooth turning activates the vignette (${vr ? "VR" : "flat"})`, {
    skip: !enabled && "set SHOCK2_E2E=1 to run",
  }, async () => {
    const root = await mkdtemp(join(tmpdir(), "shock2-smooth-test-"));
    const path = join(root, "settings.json");
    const previous = process.env.SHOCK2_SETTINGS_PATH;
    process.env.SHOCK2_SETTINGS_PATH = path;
    await writeFile(path, JSON.stringify({ vr: { turning: "Smooth", vignette: "High" } }));
    try {
      await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      const before = (await game.info()).player.rotation;
      await game.input.set("left_hand.thumbstick", [1, 0]);
      await game.step({ frames: 30 });
      const after = (await game.info()).player.rotation;
      const dot = before.reduce((sum, value, i) => sum + value * after[i]!, 0);
      const angle = 2 * Math.acos(Math.min(1, Math.abs(dot))) * 180 / Math.PI;
      assert.ok(Math.abs(angle - 45) < 0.05, `expected smooth 90 degrees/sec, got ${angle}`);
      assert.ok((await game.scene.fromSource("vr_comfort_vignette")).length > 0);
    } finally {
      if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
      else process.env.SHOCK2_SETTINGS_PATH = previous;
      await rm(root, { recursive: true, force: true });
    }
  });

  test(`Options saves from main and pause menus in ${vr ? "VR" : "flat"}`, {
    skip: !enabled && "set SHOCK2_E2E=1 to run",
  }, async () => {
    const root = await mkdtemp(join(tmpdir(), "shock2-options-test-"));
    const path = join(root, "settings.json");
    const previous = process.env.SHOCK2_SETTINGS_PATH;
    process.env.SHOCK2_SETTINGS_PATH = path;
    const click = vr ? vrClickCanvasPoint : clickCanvas;
    const debugFlags = vr ? ["--vr"] : [];
    const saved = async () => JSON.parse(await readFile(path, "utf8")) as {
      vr: { vignette: string; turning: string; snap_angle: number; reference_grid: string; grid_opacity: number };
    };
    try {
      {
        await using game = await GameServer.launch({ mission: "main_menu", debugFlags });
        await game.step({ frames: 10 });
        await click(game, [490, 202]);
        await click(game, [333, 84]); // Low -> Medium
        assert.equal((await saved()).vr.vignette, "Medium");
        await click(game, [333, 169]); // Off -> During movement
        await click(game, [333, 198]); // 30 -> 50 percent
        assert.equal((await saved()).vr.reference_grid, "DuringMovement");
        assert.equal((await saved()).vr.grid_opacity, 0.5);
        await click(game, [240, 36]);
        await click(game, [333, 113]); // 30 -> 45 degrees
        assert.equal((await saved()).vr.snap_angle, 45);
        await click(game, [333, 84]); // Snap -> Smooth
        assert.equal((await saved()).vr.turning, "Smooth");
        await click(game, DONE);
        assert.equal((await game.info()).mission, "main_menu");
      }
      {
        await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags });
        await game.step({ frames: 10 });
        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 3 });
        await click(game, pauseEntry(3));
        // This process must start with the previous process's Medium setting.
        await click(game, [333, 84]);
        assert.equal((await saved()).vr.vignette, "High");
        assert.equal((await saved()).vr.reference_grid, "DuringMovement");
        assert.equal((await saved()).vr.grid_opacity, 0.5);
        await click(game, [333, 227]); // Reset comfort, keeping turning settings
        assert.equal((await saved()).vr.reference_grid, "Off");
        assert.equal((await saved()).vr.grid_opacity, 0.3);
        assert.equal((await saved()).vr.snap_angle, 45);
        await click(game, DONE);
        assert.equal((await game.info()).paused, true);
        await click(game, pauseEntry(0));
        assert.equal((await game.info()).paused, false);
        assert.equal((await game.info()).mission, "debug_minimal");
      }
    } finally {
      if (previous === undefined) delete process.env.SHOCK2_SETTINGS_PATH;
      else process.env.SHOCK2_SETTINGS_PATH = previous;
      await rm(root, { recursive: true, force: true });
    }
  });
}
