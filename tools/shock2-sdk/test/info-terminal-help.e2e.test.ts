// Using a medsci1 info terminal unlocks the help topic in its `P$HelpText`
// mask, in a fresh mission and in a release save written before that property
// was parsed. Negative-first: with InfoComputer unregistered no topic unlocks,
// and without the save backfill the release-save terminal opens the map.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { gunzipSync } from "node:zlib";
import { GameServer, findRepoRoot } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const cases = [
  { name: "flat", vr: false },
  { name: "VR", vr: true },
  { name: "v0.0.3 save", vr: false, save: "v0.0.3/medsci1-kit" },
];

for (const { name, vr, save } of cases) {
  test(`info terminal unlocks its help topic (${name})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async (t) => {
    await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
    if (save) {
      const assetRoot = process.env.DARK_ASSET_PATH;
      assert.ok(assetRoot, "DARK_ASSET_PATH selects the save directory");
      mkdirSync(join(assetRoot, "saves"), { recursive: true });
      const saveName = `info_terminal_help_${Date.now()}`;
      const savePath = join(assetRoot, "saves", `${saveName}.sav`);
      const fixture = join(findRepoRoot(process.cwd())!, "shock2vr", "tests", "fixtures", "saves", `${save}.sav.gz`);
      writeFileSync(savePath, gunzipSync(readFileSync(fixture)));
      t.after(() => rmSync(savePath, { force: true }));
      const loaded = await game.load(saveName);
      assert.ok(loaded.success, `load failed: ${loaded.message}`);
    }
    await game.step({ frames: 10 });
    // Mission object 178 carries P$HelpText = 1: topic 0.
    const { entities } = await game.entities.list({ filter: "Info Comp", limit: 50 });
    assert.equal(entities.length, 17, "medsci1's info terminals");
    const terminal = entities.find((e) => e.template_id === 178);
    assert.ok(terminal, "info terminal 178");
    assert.deepEqual((await game.quests.list()).help_topics, []);

    const [x, , z] = terminal.position;
    await game.player.teleport({ x: x + 1.5, y: 0.9, z });
    await game.step({ frames: 10 });
    const aim = await game.player.aimAt(terminal.id, { visibility: "required" });
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    if (vr) await aimVrHandAt(game, aim.world_point, 0.35);
    const use = vr ? "right_hand.trigger" : "right_hand.squeeze";
    await game.input.set(use, 1);
    await game.step({ frames: 2 });
    await game.input.set(use, 0);
    await game.step({ frames: 4 });

    assert.deepEqual((await game.quests.list()).help_topics, [0]);
  });
}
