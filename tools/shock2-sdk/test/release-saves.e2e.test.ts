import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// What the writing build reported for each fixture (gen-save-fixtures.ts).
interface ManifestEntry {
  mission: string;
  vr: boolean;
  position: { x: number; y: number; z: number };
  quests: string[];
  inventory: string[];
}

const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
const fixtureRoot = join(repoRoot, "shock2vr", "tests", "fixtures", "saves");

// Only registered when enabled: the unit run imports every test file.
const releases = e2eEnabled
  ? readdirSync(fixtureRoot, { withFileTypes: true })
      .filter((entry) => existsSync(join(fixtureRoot, entry.name, "manifest.json")))
      .map((entry) => entry.name)
      .sort()
  : [];

// The unit test proves each fixture decodes; this one proves the real load
// path (mission parse, entity population, held items) puts the player back.
for (const tag of releases) {
  const manifest: Record<string, ManifestEntry> = JSON.parse(
    readFileSync(join(fixtureRoot, tag, "manifest.json"), "utf8"),
  );
  for (const [name, expected] of Object.entries(manifest)) {
    test(
      `${tag} save ${name} loads and restores its state`,
      { timeout: 300_000 },
      async (t) => {
        const assetRoot = process.env.DARK_ASSET_PATH;
        assert.ok(assetRoot, "DARK_ASSET_PATH must explicitly select the 25AE root");
        mkdirSync(join(assetRoot, "saves"), { recursive: true });
        const saveName = `release_save_${tag}_${name}_${Date.now()}`;
        const savePath = join(assetRoot, "saves", `${saveName}.sav`);
        const fixture = join(fixtureRoot, tag, `${name}.sav.gz`);
        writeFileSync(savePath, gunzipSync(readFileSync(fixture)));
        t.after(() => rmSync(savePath, { force: true }));

        await using game = await GameServer.launch({
          mission: "debug_minimal",
          debugFlags: expected.vr ? ["--vr"] : [],
        });
        const loaded = await game.load(saveName);
        assert.ok(loaded.success, `load failed: ${loaded.message}`);
        await game.step({ frames: 30 });

        assert.equal((await game.info()).mission, expected.mission);
        const position = await game.player.position();
        for (const axis of ["x", "y", "z"] as const) {
          assert.ok(
            Math.abs(position[axis] - expected.position[axis]) < 0.5,
            `${axis}: ${position[axis]} vs saved ${expected.position[axis]}`,
          );
        }
        const quests = (await game.quests.list()).quests
          .filter((q) => q.value === "complete")
          .map((q) => q.name)
          .sort();
        assert.deepEqual(quests, expected.quests);
        const inventory = (await game.player.inventory()).items
          .map((item) => item.name ?? "")
          .sort();
        assert.deepEqual(inventory, expected.inventory);
      },
    );
  }
}
