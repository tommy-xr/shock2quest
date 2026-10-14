import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { test } from "node:test";

import { findRepoRoot, GameServer } from "../src/index.js";
import {
  AIM_AT_PANEL, clickCanvas, DEV_ACTION, DEV_DONE, norm, panelPoint,
  pauseEntry, vrClickCanvasPoint,
} from "./helpers/frontend-menu.js";

const enabled = process.env.SHOCK2_E2E === "1";
// The twelfth slot stays in row 12 in both pickers, even with rows 1–11 empty.
const SLOT_ROW: [number, number] = [362, 63.5 + 11 * 19];

// Menu saves use fixed slots. Give each test its own writable save directory,
// while sharing the retail assets, so no click can overwrite a user's save.
function isolatedAssets(): { root: string; [Symbol.dispose](): void } {
  const previous = process.env.DARK_ASSET_PATH;
  const source = resolve(previous ?? join(findRepoRoot(process.cwd())!, "Data"));
  assert.ok(existsSync(join(source, "sshock2.kpf")), "25AE assets are required");
  const root = mkdtempSync(join(tmpdir(), "shock2-pause-save-"));
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    if (entry.name !== "saves") {
      symlinkSync(join(source, entry.name), join(root, entry.name), entry.isDirectory() ? "dir" : "file");
    }
  }
  process.env.DARK_ASSET_PATH = root;
  return {
    root,
    [Symbol.dispose]() {
      if (previous === undefined) delete process.env.DARK_ASSET_PATH;
      else process.env.DARK_ASSET_PATH = previous;
      rmSync(root, { recursive: true, force: true });
    },
  };
}

async function aim(game: GameServer, vr: boolean, point: [number, number]): Promise<void> {
  if (vr) {
    const [, y, z] = panelPoint(norm(...point));
    await game.input.set("right_hand.rotation", AIM_AT_PANEL);
    await game.input.set("right_hand.position", [0, y, z]);
  } else {
    await game.input.set("pointer.position", norm(...point));
  }
}

for (const vr of [false, true]) {
  test(`pause save slots and load preserve the live mission (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      using assets = isolatedAssets();
      const click = vr ? vrClickCanvasPoint : clickCanvas;
      const launch = (mission: string, saveFile?: string) => GameServer.launch({
        mission,
        debugFlags: [...(vr ? ["--vr"] : []), ...(saveFile ? ["--save-file", saveFile] : [])],
      });
      const pressChannel = vr ? "right_hand.trigger" : "pointer.pressed";
      let savedHitPoints: number;
      let savePath: string;
      {
        await using game = await launch("medsci1.mis");
        await game.step({ frames: 30 });
        const initial = await game.info();
        assert.ok(initial.player.entity_id !== null);
        await game.entities.sendMessage(initial.player.entity_id, { type: "Damage", amount: 3 });
        await game.step({ frames: 2 });
        savedHitPoints = (await game.info()).player.hit_points!;
        assert.ok(savedHitPoints < initial.player.hit_points!);
        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 10 });
        const paused = await game.info();

        await click(game, pauseEntry(1));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        const files = readdirSync(join(assets.root, "saves")).filter(name => name.endsWith(".sav"));
        assert.deepEqual(files, ["slot12.sav"], "Save must create the selected manual slot");
        savePath = join(assets.root, "saves", files[0]);
        const goodSave = readFileSync(savePath);
        assert.ok(goodSave.length > 1000, "slot must contain a serialized mission");
        const saved = JSON.parse(goodSave.toString("utf8"));
        assert.equal(saved.metadata.mission, "medsci1.mis");
        assert.ok(saved.metadata.played.secs > 0 || saved.metadata.played.nanos > 0,
          "save carries elapsed gameplay time for the slot label");
        assert.equal((await game.info()).paused, true);

        // Saving the selected occupied slot replaces it without adding a new slot.
        writeFileSync(savePath, "old slot contents");
        await click(game, DEV_ACTION);
        const overwritten = JSON.parse(readFileSync(savePath, "utf8"));
        assert.deepEqual(overwritten.global_data.player_vitals, saved.global_data.player_vitals);
        assert.deepEqual(overwritten.metadata, saved.metadata, "time in menus does not increase play time");
        assert.equal(readdirSync(join(assets.root, "saves")).filter(name => name.endsWith(".sav")).length, 1);

        // A filesystem refusal must leave the running game and picker intact.
        rmSync(savePath);
        mkdirSync(savePath);
        await click(game, DEV_ACTION);
        assert.equal((await game.info()).paused, true);
        assert.deepEqual((await game.info()).player, paused.player);
        rmSync(savePath, { recursive: true });
        writeFileSync(savePath, goodSave);
        await click(game, DEV_DONE);

        // A held click entering Load must not activate the new page's button.
        await aim(game, vr, pauseEntry(2));
        await game.input.set(pressChannel, 0);
        await game.step({ frames: 2 });
        await game.input.set(pressChannel, 1);
        await game.step({ frames: 2 });
        await aim(game, vr, DEV_ACTION);
        await game.step({ frames: 5 });
        assert.equal((await game.info()).paused, true, "held press cannot load on entry");
        await game.input.set(pressChannel, 0);
        await game.step({ frames: 2 });

        // Done keeps the exact running mission, and its held press cannot Continue.
        await aim(game, vr, DEV_DONE);
        await game.input.set(pressChannel, 1);
        await game.step({ frames: 2 });
        await aim(game, vr, pauseEntry(0));
        await game.step({ frames: 5 });
        assert.equal((await game.info()).paused, true, "held Done cannot Continue");
        assert.deepEqual((await game.info()).player, paused.player, "cancel preserves the live player");
        await game.input.set(pressChannel, 0);
        await game.step({ frames: 2 });

        for (const failure of ["corrupt", "missing", "missing mission"] as const) {
          await click(game, pauseEntry(2));
          await click(game, SLOT_ROW);
          if (failure === "corrupt") writeFileSync(savePath, "invalid save");
          else if (failure === "missing") rmSync(savePath);
          else {
            const unavailable = JSON.parse(goodSave.toString("utf8"));
            const mission = "missing-save-review.mis";
            unavailable.level_data[mission] = unavailable.level_data[unavailable.global_data.active_mission];
            unavailable.global_data.active_mission = mission;
            unavailable.metadata.mission = mission;
            writeFileSync(savePath, JSON.stringify(unavailable));
          }
          await click(game, DEV_ACTION);
          const failed = await game.info();
          assert.equal(failed.paused, true, `${failure} save leaves the menu open`);
          assert.equal(failed.mission, "medsci1.mis");
          assert.deepEqual(failed.player, paused.player, `${failure} save preserves the live player`);
          writeFileSync(savePath, goodSave);
          await click(game, DEV_DONE);
        }

        // Change the live state, then recover it through the picker.
        await click(game, pauseEntry(0));
        await game.entities.sendMessage((await game.info()).player.entity_id!, { type: "Damage", amount: 2 });
        await game.step({ frames: 2 });
        assert.ok((await game.info()).player.hit_points! < savedHitPoints);
        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 5 });
        await click(game, pauseEntry(2));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        const loaded = await game.info();
        assert.equal(loaded.paused, false, "successful load resumes gameplay");
        assert.equal(loaded.player.hit_points, savedHitPoints);
      }

      // A second runtime proves the slot is read from disk across process lifetimes.
      {
        await using game = await launch("earth.mis");
        await game.step({ frames: 5 });
        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 5 });
        await click(game, pauseEntry(2));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        const restored = await game.info();
        assert.equal(restored.mission, "medsci1.mis");
        assert.equal(restored.paused, false);
        assert.equal(restored.player.hit_points, savedHitPoints);
      }

      // Command-line resume must preserve prior campaign time when saving again.
      const prior = JSON.parse(readFileSync(savePath!, "utf8"));
      prior.metadata.played = { secs: 3723, nanos: 0 };
      writeFileSync(savePath!, JSON.stringify(prior));
      {
        await using game = await launch("earth.mis", savePath!);
        await game.step({ frames: 30 });
        assert.equal((await game.info()).mission, "medsci1.mis");
        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 5 });
        await click(game, pauseEntry(1));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        const resumed = JSON.parse(readFileSync(savePath!, "utf8"));
        assert.ok(resumed.metadata.played.secs >= 3723, "startup load retains campaign play time");
        assert.equal(resumed.metadata.mission, "medsci1.mis");
      }
    });
}
