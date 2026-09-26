import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { test } from "node:test";

import { GameServer, HttpError, findRepoRoot } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
const saveDir = () =>
  join(
    process.env.DARK_ASSET_PATH ??
      join(findRepoRoot(process.cwd()) ?? process.cwd(), "Data"),
    "saves",
  );

test(
  "QuickSave and QuickLoad in a generated debug scene leave it running",
  { skip: !enabled, timeout: 600_000 },
  async () => {
    const saves = saveDir();
    const quickSave = join(saves, "save1.sav");
    const parked = join(saves, `save1-${randomUUID()}.sav`);
    const hadQuickSave = existsSync(quickSave);
    mkdirSync(saves, { recursive: true });
    if (hadQuickSave) renameSync(quickSave, parked);

    try {
      await using game = await GameServer.launch({ mission: "debug_weapons" });
      await game.step({ frames: 2 });
      await assert.rejects(
        game.save(`debug-scene-${randomUUID()}`),
        (error: unknown) =>
          error instanceof HttpError &&
          error.status === 409 &&
          JSON.parse(error.body).error_code === "unsupported_scene",
      );
      await game.input.trigger("QuickSave");
      await game.step({ frames: 2 });
      await game.input.trigger("QuickLoad");
      await game.step({ frames: 2 });

      assert.equal((await game.health()).status, "ok");
      assert.equal((await game.info()).mission, "debug_weapons");
      assert.equal(existsSync(quickSave), false, "debug scenes must not produce unloadable saves");
      assert.ok(game.logs().some((line) => line.includes("Cannot save debug scene")));
    } finally {
      rmSync(quickSave, { force: true });
      if (hadQuickSave) renameSync(parked, quickSave);
    }
  },
);

test(
  "a previously written debug-scene save fails safely when loaded",
  { skip: !enabled, timeout: 600_000 },
  async () => {
    const saves = saveDir();
    const sourceName = `issue-1398-source-${randomUUID()}`;
    const debugName = `issue-1398-legacy-debug-${randomUUID()}`;
    const sourcePath = join(saves, `${sourceName}.sav`);
    const debugPath = join(saves, `${debugName}.sav`);

    try {
      await using game = await GameServer.launch({ mission: "medsci1.mis" });
      await game.step({ frames: 2 });
      await game.save(sourceName);
      const oldSave = JSON.parse(readFileSync(sourcePath, "utf8"));
      oldSave.global_data.active_mission = "debug_weapons";
      writeFileSync(debugPath, JSON.stringify(oldSave));

      await assert.rejects(
        game.load(debugName),
        (error: unknown) =>
          error instanceof HttpError &&
          error.status === 500 &&
          error.body.includes("Cannot load debug scene 'debug_weapons'"),
      );
      assert.equal((await game.health()).status, "ok");
      assert.equal((await game.info()).mission, "medsci1.mis");
    } finally {
      rmSync(sourcePath, { force: true });
      rmSync(debugPath, { force: true });
    }
  },
);
