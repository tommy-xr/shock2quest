import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { GameServer, HttpError, findRepoRoot } from "../src/index.js";
import type { Position } from "../src/types.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";

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

interface PlayerVitals {
  hitPoints: number;
  maxHitPoints: number;
  psiPoints: number;
  maxPsiPoints: number;
}

function distance(a: Position, b: Position): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

function playerVitals(
  info: Awaited<ReturnType<GameServer["info"]>>,
): PlayerVitals {
  const {
    hit_points: hitPoints,
    max_hit_points: maxHitPoints,
    psi_points: psiPoints,
    max_psi_points: maxPsiPoints,
  } = info.player;
  assert.notEqual(hitPoints, null, "player should have current hit points");
  assert.notEqual(maxHitPoints, null, "player should have maximum hit points");
  assert.notEqual(psiPoints, null, "player should have current psi points");
  assert.notEqual(maxPsiPoints, null, "player should have maximum psi points");
  return {
    hitPoints: hitPoints as number,
    maxHitPoints: maxHitPoints as number,
    psiPoints: psiPoints as number,
    maxPsiPoints: maxPsiPoints as number,
  };
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

test(
  "saving an unsupported player returns a structured pose refusal",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `unsupported_save_e2e_${Date.now()}`;
    t.after(() => {
      const path = findSavePath(saveName);
      if (path) rmSync(path, { force: true });
    });

    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 5,
    });
    await game.step({ frames: 5 });

    const start = await game.player.position();
    await game.player.teleport({ x: start.x, y: -100, z: start.z });
    await game.step({ frames: 3 });
    const unsupported = await game.player.position();

    await assert.rejects(
      game.save(saveName),
      (error: unknown) => {
        if (!(error instanceof HttpError) || error.status !== 409) return false;
        const body = JSON.parse(error.body) as {
          success: boolean;
          error_code: string;
          reason: string;
          player_pose: {
            position: [number, number, number];
            is_crouched: boolean;
          };
        };
        assert.equal(body.success, false);
        assert.equal(body.error_code, "unsupported_player_pose");
        assert.match(body.reason, /support/i);
        assert.equal(body.player_pose.is_crouched, false);
        assert.ok(
          Math.abs(body.player_pose.position[0] - unsupported.x) < 0.01 &&
            Math.abs(body.player_pose.position[1] - unsupported.y) < 0.01 &&
            Math.abs(body.player_pose.position[2] - unsupported.z) < 0.01,
          `refusal must report the live pose: ${JSON.stringify(body.player_pose)} vs ${JSON.stringify(unsupported)}`,
        );
        return true;
      },
      "an unsupported pose must be an explicit 409 with its reason and live pose",
    );

    assert.equal(
      (await game.info()).mission,
      "medsci1.mis",
      "a refused save must leave the runtime live",
    );
  },
);

test(
  "current and maximum player vitals survive mission transition and cross-process save/load",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Negative-first for #551: Earth Psionic Training gives us a production
    // path to non-default PSI and HP. The authored room entry sets PSI to 5,
    // then a real psi-amp burnout spends one point and deals three damage.
    // Before the fix, the transition below resets 27/30 HP + 4/50 PSI to the
    // destination's template defaults, and a fresh-process load does likewise.
    const saveName = `player_vitals_e2e_${Date.now()}`;
    let expected: PlayerVitals;

    {
      await using game = await GameServer.launch({
        mission: "earth.mis",
        port: basePort + 3,
      });
      await game.step({ frames: 5 });

      await crossEarthTrainingTripwire(game, 320, 325);
      const [amp] = await game.entities.byTemplate(290);
      assert.ok(amp, "Earth Psionic Training should contain its authored Psi Amp");
      await earthWorldUse(game, amp);
      assert.equal(
        (await game.info()).player.wielded_entity_id,
        amp.id,
        "normal world-use should wield the authored Psi Amp",
      );

      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 130 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 45 });

      expected = playerVitals(await game.info());
      assert.deepEqual(
        expected,
        {
          hitPoints: 27,
          maxHitPoints: 30,
          psiPoints: 4,
          maxPsiPoints: 50,
        },
        "the test setup should establish non-default current HP and PSI",
      );

      await game.transitionLevel("medsci1.mis");
      await game.step({ frames: 5 });
      assert.equal((await game.info()).mission, "medsci1.mis");
      assert.deepEqual(
        playerVitals(await game.info()),
        expected,
        "an ordinary mission transition should carry exact current and maximum vitals",
      );

      const saveResult = await game.save(saveName);
      assert.equal(saveResult.success, true);
      assert.equal(saveResult.mission, "medsci1.mis");
    }

    {
      await using game = await GameServer.launch({
        mission: "eng1.mis",
        port: basePort + 4,
      });
      await game.step({ frames: 2 });
      assert.equal((await game.info()).mission, "eng1.mis");

      const loadResult = await game.load(saveName);
      assert.equal(loadResult.success, true);
      assert.equal(loadResult.mission, "medsci1.mis");
      assert.deepEqual(
        playerVitals(await game.info()),
        expected,
        "a fresh runtime process should restore exact current and maximum vitals",
      );
    }
  },
);
