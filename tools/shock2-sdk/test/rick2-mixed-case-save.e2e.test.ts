import assert from "node:assert/strict";
import { existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { findRepoRoot, GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const reproSave = process.env.SHOCK2_RICK2_MIXED_CASE_SAVE;
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

async function turret709(game: GameServer) {
  return (await game.entities.list({ limit: 10_000 })).entities.find(
    (entity) => entity.template_id === 709,
  );
}

// Negative-first: on the old runtime this creates level_data["Rick2.mis"],
// then the fresh process looks up only "rick2.mis" and reconstructs turret709
// from the pristine mission. The test owns its save and needs no campaign data.
test(
  "Rick2.mis: a destroyed mission entity stays absent across a fresh process",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `rick2_mixed_case_${Date.now()}`;
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
        await game.entities.sendMessage(turret.id, {
          type: "Damage",
          amount: 1_000,
        });
        await game.step({ frames: 5 });
        assert.equal(
          await turret709(game),
          undefined,
          "ordinary lethal Damage must remove mission turret709 before save",
        );
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
        assert.equal(
          await turret709(game),
          undefined,
          "the fresh process must use the saved Rick2 entity snapshot",
        );
      }
    } finally {
      const path = savePath(saveName);
      if (path) rmSync(path);
    }
  },
);

// The private checkpoint was written immediately after mission turret709 was
// destroyed with ordinary Crystal Shard edges. Its active mission and current
// entity snapshot are both keyed `Rick2.mis`. Before #905, cold load lowercased
// only the lookup, missed that exact snapshot, and silently rebuilt pristine
// mission entities even though global player state restored correctly.
test(
  "Rick2.mis: cold load restores a mixed-case mission entity snapshot",
  {
    skip: !e2eEnabled || !reproSave || !existsSync(reproSave),
    timeout: 600_000,
  },
  async () => {
    assert.ok(
      reproSave,
      "SHOCK2_RICK2_MIXED_CASE_SAVE must name the private repro save",
    );
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: basePort + 2,
      debugFlags: ["--save-file", reproSave],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      repoRoot: process.env.SHOCK2_REPO_ROOT,
    });
    await game.step({ frames: 10 });

    const info = await game.info();
    const position = await game.player.position();
    assert.equal(info.mission, "Rick2.mis");
    assert.equal(info.player.hit_points, 24);
    assert.equal(info.player.stats?.cyber_modules, 175);
    assert.ok(
      Math.hypot(
        position.x - 156.13382,
        position.y - 92.82399,
        position.z + 10.840119,
      ) < 0.2,
      `global player pose must still restore exactly: ${JSON.stringify(position)}`,
    );

    const restoredTurret709 = await turret709(game);
    assert.equal(
      restoredTurret709,
      undefined,
      `destroyed mission turret709 must remain absent after cold load; got ${JSON.stringify(restoredTurret709)}`,
    );
  },
);
