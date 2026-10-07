import assert from "node:assert/strict";
import { test } from "node:test";
import { launchDeveloperGame } from "./helpers/developer-game.js";
import {
  DEV_ACTION, DEV_DONE, PAUSE_DEVELOPER, pauseEntry,
  clickCanvas, vrClickCanvasPoint,
} from "./helpers/frontend-menu.js";

// Rows on the shared GAMELODR.BIN canvas; both fit on the first page.
const MAX_ALCOHOL: [number, number] = [362, 54 + 11 * 19 + 9.5];
const CLEAR_ALCOHOL: [number, number] = [362, 54 + 12 * 19 + 9.5];

for (const vr of [false, true]) {
  test(`alcohol cheats immediately maximize and clear the effect (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async (t) => {
    await using game = await launchDeveloperGame(t, {
      mission: "earth.mis",
      repoRoot: process.env.SHOCK2_E2E_REPO_ROOT,
      debugFlags: vr ? ["--vr"] : [],
    });
    const click = (point: [number, number]) =>
      (vr ? vrClickCanvasPoint : clickCanvas)(game, point);
    const vital = async () => (await game.info()).player;
    async function openCheats() {
      await game.input.trigger("TogglePauseMenu");
      await game.step({ frames: 3 });
      await click(PAUSE_DEVELOPER);
      await click(DEV_ACTION);
    }
    async function resume() {
      await click(DEV_DONE);
      await click(DEV_DONE);
      await click(pauseEntry(0));
    }
    await game.step({ frames: 3 });
    const before = await vital();
    await openCheats();
    await click(MAX_ALCOHOL);
    assert.equal((await game.info()).paused, true);
    assert.equal((await vital()).alcohol_level, 6);
    assert.equal((await vital()).alcohol_intensity, 1);
    await game.step({ frames: 60 });
    assert.equal((await vital()).alcohol_level, 6, "paused cheat must not decay");
    await resume();
    await game.step({ frames: 60 });
    assert.ok((await vital()).alcohol_level < 6, "normal recovery resumes");
    assert.ok((await game.scene.fromSource("alcohol_overlay")).length > 0);
    await openCheats();
    await click(CLEAR_ALCOHOL);
    assert.equal((await vital()).alcohol_level, 0);
    assert.equal((await vital()).alcohol_intensity, 0, "clear removes the fade immediately");
    await resume();
    assert.equal((await game.scene.fromSource("alcohol_overlay")).length, 0);
    const after = await vital();
    assert.equal(after.hit_points, before.hit_points, "cheats do not consume an item");
    assert.equal(after.psi_points, before.psi_points);
  });
}
