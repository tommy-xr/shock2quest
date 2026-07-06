import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { GameServer, HttpError, findRepoRoot } from "../src/index.js";
import type { Position } from "../src/types.js";

// End-to-end test for the save-to-file / load-from-file endpoints. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before POST /v1/save + /v1/load existed, there was no way to
// persist a game from HTTP and resume it in a later runtime launch. The whole
// point is cross-launch resume for the automated play-through loop (a "frontier"
// save reloaded after a relaunch). This test proves exactly that: it saves in
// one runtime process, shuts it down, launches a FRESH process on a DIFFERENT
// mission, loads the save, and asserts the active mission + player position are
// restored - which is impossible without the endpoints (the save call 404s).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8101);

function distance(a: Position, b: Position): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

// Locate the on-disk .sav for a freshly-saved name by scanning the candidate
// saves dirs (mirrors shock2vr::save_file_path = <data_root>/saves/<name>.sav;
// data_root is DARK_ASSET_PATH or a ./Data search relative to the repo root).
function findSavePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((d): d is string => Boolean(d));
  for (const root of roots) {
    const p = join(root, "saves", `${saveName}.sav`);
    if (existsSync(p)) return p;
  }
  return undefined;
}

test(
  "save then load in a fresh runtime restores mission and player position",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Use a unique save name per run so stale files from a prior run can't mask
    // a regression (a real save must be written this run for load to succeed).
    const saveName = `frontier_e2e_${Date.now()}`;
    let savedPos: Position;

    // --- Session 1: start in medsci1, move away from spawn, save. ---
    {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        port: basePort,
      });

      await game.step({ frames: 2 });
      assert.equal(
        (await game.info()).mission,
        "medsci1.mis",
        "should start in medsci1",
      );

      // Walk forward so the saved position is distinct from the default spawn -
      // this makes the position-restore assertion meaningful (not a coincidence
      // of both sessions spawning at the same place).
      const spawnPos = await game.player.position();
      await game.input.set("right_hand.thumbstick", [0.0, 1.0]);
      await game.step({ frames: 90 });
      await game.input.set("right_hand.thumbstick", [0.0, 0.0]);
      await game.step({ frames: 5 });

      savedPos = await game.player.position();
      assert.ok(
        distance(savedPos, spawnPos) > 0.5,
        `player should have moved from spawn before saving (spawn=${JSON.stringify(spawnPos)}, saved=${JSON.stringify(savedPos)})`,
      );

      const saveResult = await game.save(saveName);
      assert.equal(saveResult.success, true, "save should report success");
      assert.equal(saveResult.mission, "medsci1.mis");
    }

    // --- Session 2: a FRESH process on a DIFFERENT mission, then load. ---
    {
      await using game = await GameServer.launch({
        mission: "eng1.mis",
        port: basePort + 1,
      });

      await game.step({ frames: 2 });
      assert.equal(
        (await game.info()).mission,
        "eng1.mis",
        "fresh session should start on the different mission (eng1)",
      );

      const loadResult = await game.load(saveName);
      assert.equal(loadResult.success, true, "load should report success");
      assert.equal(
        loadResult.mission,
        "medsci1.mis",
        "load result should report the restored mission",
      );

      // The load is synchronous, so info() reflects the restored mission right
      // away (no step needed to settle the scene swap).
      assert.equal(
        (await game.info()).mission,
        "medsci1.mis",
        "active mission should be the loaded medsci1 (cross-launch resume)",
      );

      const loadedPos = await game.player.position();
      assert.ok(
        distance(loadedPos, savedPos) < 1.0,
        `restored player position should match the saved one (saved=${JSON.stringify(savedPos)}, loaded=${JSON.stringify(loadedPos)})`,
      );

      // Bad names must be rejected with a 400 (not crash the game-loop thread):
      // the save/load path builds a filesystem path from the name, so an empty
      // or path-like name has to be refused at the edge.
      await assert.rejects(
        game.save(""),
        (err: unknown) =>
          err instanceof HttpError && err.status === 400,
        "empty save name should be a 400",
      );
      await assert.rejects(
        game.load("../escape"),
        (err: unknown) =>
          err instanceof HttpError && err.status === 400,
        "path-like save name should be a 400",
      );
      // Loading a save that does not exist must 404, not panic the runtime.
      await assert.rejects(
        game.load(`nonexistent_${Date.now()}`),
        (err: unknown) =>
          err instanceof HttpError && err.status === 404,
        "loading a missing save should be a 404",
      );

      // Runtime is still live after the rejected calls.
      assert.equal(
        (await game.info()).mission,
        "medsci1.mis",
        "runtime should stay live and on medsci1 after rejected save/load calls",
      );
    }
  },
);

test(
  "loading a corrupt save returns 500 without bricking the runtime",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // A frontier save persists across code changes, so an existing-but-corrupt
    // or schema-incompatible .sav is a real hazard: the load path unwraps while
    // parsing. The runtime must catch that and return an error, NOT unwind out
    // of the game-loop thread (which would hang every later command). Negative-
    // first: without the catch this load hangs/bricks and the liveness check
    // below times out.
    const saveName = `corrupt_e2e_${Date.now()}`;

    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 2,
    });
    await game.step({ frames: 2 });

    // Write a valid save so the file exists (and to locate the saves dir), then
    // overwrite it with garbage that is not a valid SaveData payload.
    await game.save(saveName);
    const savePath = findSavePath(saveName);
    assert.ok(savePath, `should be able to locate the saved file for ${saveName}`);
    writeFileSync(savePath, "this is not a valid save file");

    await assert.rejects(
      game.load(saveName),
      (err: unknown) => err instanceof HttpError && err.status === 500,
      "loading a corrupt save should be a 500, not a hang/crash",
    );

    // The decisive check: the runtime is still responsive and on the original
    // mission (the failed load left the previous scene intact).
    assert.equal(
      (await game.info()).mission,
      "medsci1.mis",
      "runtime should stay live on medsci1 after a failed load",
    );
  },
);
