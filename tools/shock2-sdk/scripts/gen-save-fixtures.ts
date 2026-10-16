// Writes save fixtures from a released build for the shock2vr save-compat unit
// test and release-saves.e2e.test.ts. Build the tag's runtime in its own
// worktree, then:
//
//   node dist/scripts/gen-save-fixtures.js v0.0.3 <worktree>/target/debug/debug_runtime
//
// Each checkpoint is saved by that build and gzipped into
// shock2vr/tests/fixtures/saves/<tag>/<name>.sav.gz; manifest.json records
// what the build itself reported for it (position, quests, inventory).

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

import { findRepoRoot, GameServer } from "../src/index.js";

type Game = Awaited<ReturnType<typeof GameServer.launch>>;

interface Checkpoint {
  name: string;
  mission: string;
  vr?: boolean;
  setup: (game: Game) => Promise<void>;
}

// Carried items and a raised character sheet.
async function medsciKit(game: Game): Promise<void> {
  await game.player.spawnItem("Shotgun");
  await game.player.spawnItem("20 Nanites");
  await game.player.setStats({ strength: 3, agility: 2 });
}

const CHECKPOINTS: Checkpoint[] = [
  { name: "earth-start", mission: "earth.mis", setup: async () => {} },
  {
    // Mid-training: career and year live in quest bits.
    name: "station-navy-year2",
    mission: "station.mis",
    setup: async (game) => {
      await game.quests.set("career_navy", "complete");
      await game.quests.set("training_year_2", "complete");
    },
  },
  { name: "medsci1-kit", mission: "medsci1.mis", setup: medsciKit },
  // VR persists hand/belt state the flat path never writes.
  { name: "medsci1-kit-vr", mission: "medsci1.mis", vr: true, setup: medsciKit },
  // Later decks author property types the early ones don't.
  { name: "eng1-start", mission: "eng1.mis", setup: async () => {} },
  { name: "command1-start", mission: "command1.mis", setup: async () => {} },
];

const [tag, binary] = process.argv.slice(2);
if (!tag || !binary) {
  console.error("usage: gen-save-fixtures <tag> <debug_runtime binary>");
  process.exit(2);
}
const assetRoot = process.env.DARK_ASSET_PATH;
if (!assetRoot) throw new Error("DARK_ASSET_PATH must point at the game data");

const repoRoot = findRepoRoot(dirname(fileURLToPath(import.meta.url)));
if (!repoRoot) throw new Error("run from inside the shock2quest checkout");
const outDir = join(repoRoot, "shock2vr", "tests", "fixtures", "saves", tag);
mkdirSync(outDir, { recursive: true });

const manifest: Record<string, unknown> = {};
for (const { name, mission, vr, setup } of CHECKPOINTS) {
  await using game = await GameServer.launch({
    mission,
    runtimeBinary: binary,
    debugFlags: vr ? ["--vr"] : [],
  });
  await game.step({ frames: 30 });
  await setup(game);
  await game.step({ frames: 30 });

  const saveName = `compat-fixture-${tag}-${name}`;
  const savePath = join(assetRoot, "saves", `${saveName}.sav`);
  try {
    const saved = await game.save(saveName);
    if (!saved.success) throw new Error(`${name}: save failed: ${saved.message}`);
    writeFileSync(join(outDir, `${name}.sav.gz`), gzipSync(readFileSync(savePath)));
  } finally {
    // The runtime saves into the player's own save folder.
    rmSync(savePath, { force: true });
  }

  const position = await game.player.position();
  const quests = (await game.quests.list()).quests
    .filter((q) => q.value === "complete")
    .map((q) => q.name)
    .sort();
  const inventory = (await game.player.inventory()).items
    .map((item) => item.name ?? "")
    .sort();
  manifest[name] = { mission, vr: vr ?? false, position, quests, inventory };
  // Per checkpoint, so a failed run leaves a manifest of what it finished.
  writeFileSync(join(outDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`${tag}/${name}: ${mission}`);
}
