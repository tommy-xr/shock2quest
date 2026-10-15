import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { GameServer, supportedModConfigurations, type FrameSnapshot } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

function assertIdentity(info: FrameSnapshot, campaign: "base" | "scp", mission: string) {
  const expectedGamesys = campaign === "scp" ? "shockscp.gam" : "shock2.gam";
  assert.equal(info.campaign_identity?.campaign, campaign);
  assert.equal(info.campaign_identity?.loaded_gamesys, expectedGamesys);
  assert.equal(info.campaign_identity?.mission.mission, mission);
  assert.equal(info.campaign_identity?.mission.gamesys, expectedGamesys);
  assert.equal(info.campaign_identity?.mission.quest_variables.scplevel, campaign === "scp" ? 1 : undefined);
}

for (const configuration of supportedModConfigurations) {
  for (const presentation of ["flat", "vr"] as const) {
    test(`${configuration.name} ${presentation}: campaign identity, transitions, and save isolation`,
      { skip: !enabled, timeout: 600_000 }, async () => {
        const root = process.env.DARK_ASSET_PATH;
        assert.ok(root, "verification requires an explicit 25AE DARK_ASSET_PATH");
        const save = `campaign_e2e_${configuration.name}_${presentation}_${Date.now()}`;
        const savePath = join(root, "saves", `${save}.sav`);
        const debugFlags = presentation === "vr" ? ["--vr"] : [];
        const other = supportedModConfigurations.find(c => c.name !== configuration.name)!;
        try {
          {
            await using game = await GameServer.launch({ mission: "medsci1.mis", mods: configuration.mods, debugFlags });
            await game.step({ frames: 60 });
            const info = await game.info();
            assert.deepEqual(info.mods, configuration.mods);
            assertIdentity(info, configuration.name, "medsci1.mis");
            await game.save(save);
            await game.transitionLevel("medsci2.mis");
            await game.step({ frames: 2 });
            assertIdentity(await game.info(), configuration.name, "medsci2.mis");
            await game.load(save);
            assertIdentity(await game.info(), configuration.name, "medsci1.mis");
          }
          {
            await using game = await GameServer.launch({ mission: "medsci2.mis", mods: other.mods, debugFlags });
            await game.step({ frames: 2 });
            const before = await game.info();
            await assert.rejects(game.load(save), /save uses .* campaign/);
            const after = await game.info();
            assert.equal(after.mission, before.mission);
            assert.deepEqual(after.campaign_identity, before.campaign_identity);
            assert.deepEqual(after.player.position, before.player.position);
          }
          await assert.rejects(
            GameServer.launch({ mission: "medsci1.mis", mods: other.mods, debugFlags: [...debugFlags, "--save-file", savePath] }),
            /save uses .* campaign/,
          );
        } finally {
          rmSync(savePath, { force: true });
        }
      });
  }
}


test("SCP load rejects mission fingerprints and preserves the live game", { skip: !enabled, timeout: 300_000 }, async () => {
  const installed = process.env.DARK_ASSET_PATH;
  assert.ok(installed, "explicit 25AE root required");
  for (const sentinel of ["sshock2.kpf", "mods/scp.kpf", "mods/sshock2ee.kpf"]) {
    assert.ok(existsSync(join(installed, sentinel)), sentinel);
  }
  const root = mkdtempSync(join(tmpdir(), "shock2-campaign-mismatch-"));
  try {
    for (const entry of readdirSync(installed, { withFileTypes: true })) {
      if (entry.name !== "saves") symlinkSync(join(installed, entry.name), join(root, entry.name), entry.isDirectory() ? "junction" : "file");
    }
    mkdirSync(join(root, "saves"));
    // A valid chunk table with a base GAM_FILE; preflight must reject it before geometry is read.
    const bytes = Buffer.alloc(344);
    bytes.writeUInt32LE(320, 0);
    bytes.write("GAM_FILE", 272);
    bytes.write("shock2.gam\0", 296);
    bytes.writeUInt32LE(1, 320);
    bytes.write("GAM_FILE", 324);
    bytes.writeUInt32LE(272, 336);
    bytes.writeUInt32LE(11, 340);
    writeFileSync(join(root, "mismatch.mis"), bytes);
    console.log(JSON.stringify({ asset_root: root, sentinel: realpathSync(join(root, "sshock2.kpf")), scp: realpathSync(join(root, "mods/scp.kpf")) }));
    process.env.DARK_ASSET_PATH = root;
    await using game = await GameServer.launch({ mission: "medsci1.mis", mods: ["scp"] });
    await game.step({ frames: 60 });
    await game.save("mismatched_campaign_fixture");
    const file = join(root, "saves", "mismatched_campaign_fixture.sav");
    const saved = JSON.parse(readFileSync(file, "utf8"));
    saved.global_data.active_mission = "mismatch.mis";
    writeFileSync(file, JSON.stringify(saved));
    const before = await game.info();
    await assert.rejects(game.load("mismatched_campaign_fixture"), /requires shockscp.gam/);
    const after = await game.info();
    assert.deepEqual(after.campaign_identity, before.campaign_identity);
    assert.equal(after.mission, before.mission);
    assert.deepEqual(after.player.position, before.player.position);
    assert.equal((await game.step({ frames: 1 })).frames_advanced, 1);
  } finally {
    process.env.DARK_ASSET_PATH = installed;
    rmSync(root, { recursive: true, force: true });
  }
});
