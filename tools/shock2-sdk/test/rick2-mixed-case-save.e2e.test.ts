import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { findRepoRoot, GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8295);

function savePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

async function model(game: GameServer, id: number) {
  return (await game.entities.detail(id)).properties.find((p) => p.name === "Model")?.value;
}

async function turret709(game: GameServer) {
  return (await game.entities.list({ limit: 10_000 })).entities.find(
    (entity) => entity.template_id === 709,
  );
}

// Negative-first: on the old runtime this creates level_data["Rick2.mis"],
// then the fresh process looks up only "rick2.mis" and reconstructs turret709
// from the pristine mission. The test owns its save and needs no campaign data.
test(
  "Rick2.mis: a destroyed mission entity stays destroyed across a fresh process",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `rick2_mixed_case_${Date.now()}`;
    let shell: unknown;
    try {
      {
        await using game = await GameServer.launch({
          mission: "earth.mis",
          port: basePort,
          echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
          repoRoot: process.env.SHOCK2_REPO_ROOT,
        });
        assert.equal((await game.transitionLevel("Rick2.mis")).success, true);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).mission, "Rick2.mis");

        const turret = await turret709(game);
        assert.ok(turret, "Rick2 must contain mission turret709 before damage");
        const pristine = await model(game, turret.id);
        await game.entities.sendMessage(turret.id, {
          type: "Damage",
          amount: 1_000,
        });
        await game.step({ frames: 5 });
        // CameraDeath (#2102) keeps the destroyed turret's damaged shell.
        const destroyed = await turret709(game);
        assert.ok(destroyed, "a destroyed Rick turret keeps its shell");
        shell = await model(game, destroyed.id);
        assert.notEqual(shell, pristine, "ordinary lethal Damage must destroy mission turret709 before save");
        assert.equal((await game.save(saveName)).success, true);
      }

      {
        await using game = await GameServer.launch({
          mission: "earth.mis",
          port: basePort + 1,
          echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
          repoRoot: process.env.SHOCK2_REPO_ROOT,
        });
        const loaded = await game.load(saveName);
        assert.equal(loaded.success, true);
        assert.equal(loaded.mission, "Rick2.mis");
        await game.step({ frames: 5 });
        const restored = await turret709(game);
        assert.ok(restored, "the destroyed shell survives the save");
        assert.equal(
          await model(game, restored.id),
          shell,
          "the fresh process must use the saved Rick2 entity snapshot, not the pristine turret",
        );
      }
    } finally {
      const path = savePath(saveName);
      if (path) rmSync(path);
    }
  },
);
