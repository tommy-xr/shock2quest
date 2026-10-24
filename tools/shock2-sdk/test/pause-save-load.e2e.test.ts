import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { dataRoot } from "./helpers/crf.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";

import { GameServer } from "../src/index.js";
import {
  AIM_AT_PANEL, clickCanvas, DEV_ACTION, DEV_DONE, norm, panelPoint,
  pauseEntry, vrClickCanvasPoint,
} from "./helpers/frontend-menu.js";

const enabled = process.env.SHOCK2_E2E === "1";
// The twelfth slot stays in row 12 in both pickers, even with rows 1–11 empty.
const SLOT_ROW: [number, number] = [362, 63.5 + 11 * 19];

// Menu saves use fixed slots. Give each test its own writable save directory,
// while sharing the retail assets, so no click can overwrite a user's save.
function isolatedAssets() {
  assert.ok(existsSync(join(dataRoot(), "sshock2.kpf")), "25AE assets are required");
  return isolatedDataRoot(["saves", "recordings"]);
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

async function finishLoading(game: GameServer, vr: boolean): Promise<void> {
  assert.equal((await game.info()).mission, "loading", "save loads must show the loading screen");
  const { objects } = await game.scene.objects();
  assert.equal(objects.length, 3, "only the loading backdrop, disc and progress bar should draw");
  for (const object of objects) {
    const distance = Math.hypot(...object.position);
    assert.ok(vr ? distance > 1 : distance === 0,
      "loading canvas must use the correct flat/VR presentation");
  }
  await game.waitFor(async () => (await game.info()).mission !== "loading", {
    timeoutMs: 60_000, description: "save load completion",
  });
}

test("QuickLoad interrupts a fixed-step batch before a failed load", {
  skip: !enabled, timeout: 300_000,
}, async () => {
  using assets = isolatedAssets();
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
    debugFlags: ["--defer-transitions", "--experimental", "fixed_simulation"],
  });
  await game.step({ frames: 30 });
  await game.input.trigger("ToggleInputRecording");
  await game.step({ frames: 1 });
  await game.input.trigger("ToggleInputRecording");
  await game.step({ frames: 1 });

  const recordings = join(assets.root, "recordings");
  const recording = join(recordings, readdirSync(recordings).find(name => name.endsWith(".jsonl"))!);
  const [header, frame] = readFileSync(recording, "utf8").trim().split("\n").map(line => JSON.parse(line));
  const baseline = JSON.parse(readFileSync(join(recordings, header.save), "utf8"));
  // Replay supplies a long render frame, forcing eight fixed ticks. QuickLoad
  // starts on the first tick; the remaining ticks must wait for loading to end.
  frame.total = frame.total - frame.dt + 8 / 60;
  frame.dt = 8 / 60;
  frame.triggered = ["QuickLoad"];
  frame.held = [];
  frame.input.right_hand.thumbstick = { x: 0, y: 1 };
  writeFileSync(recording, `${JSON.stringify(header)}\n${JSON.stringify(frame)}\n`);
  mkdirSync(join(assets.root, "saves"), { recursive: true });
  writeFileSync(join(assets.root, "saves", "save1.sav"), "invalid save");
  await game.replay(recording);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).mission, "loading");

  assert.equal((await game.save("interrupted-tick")).mission, "medsci1.mis");
  const interrupted = JSON.parse(readFileSync(join(assets.root, "saves", "interrupted-tick.sav"), "utf8"));
  assert.equal(interrupted.metadata.mission, interrupted.global_data.active_mission);
  const seconds = (time: { secs: number; nanos: number }) => time.secs + time.nanos / 1e9;
  const simulated = seconds(interrupted.metadata.played) - seconds(baseline.metadata.played);
  assert.ok(simulated > 0.01 && simulated < 0.04,
    `QuickLoad must stop after one tick, simulated ${simulated} seconds`);

  await game.step({ frames: 3 });
  await game.save("loading-frozen");
  const frozen = JSON.parse(readFileSync(join(assets.root, "saves", "loading-frozen.sav"), "utf8"));
  assert.deepEqual(frozen.metadata.played, interrupted.metadata.played);
  assert.deepEqual(frozen.global_data, interrupted.global_data, "loading must preserve the retained player");
  await finishLoading(game, false);
  assert.equal((await game.info()).mission, "medsci1.mis");
});

for (const vr of [false, true]) {
  for (const changeMission of [false, true]) {
    test(`menu load restores ${changeMission ? "another" : "the same"} mission's entities (${vr ? "VR" : "flat"})`,
      { skip: !enabled, timeout: 300_000 }, async () => {
        using assets = isolatedAssets();
        await using game = await GameServer.launch({
          mission: "earth.mis",
          debugFlags: ["--defer-transitions", ...(vr ? ["--vr"] : [])],
        });
        const click = vr ? vrClickCanvasPoint : clickCanvas;
        await game.step({ frames: 30 });
        const originalCount = (await game.entities.byTemplate(-397)).length;
        if (changeMission) {
          for (let i = 0; i < 2; i++) {
            await game.input.trigger("SpawnDebugMonster");
            await game.step({ frames: 1 });
          }
        }
        const savedCount = originalCount + (changeMission ? 2 : 0);
        assert.equal((await game.entities.byTemplate(-397)).length, savedCount);

        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 5 });
        await click(game, pauseEntry(1));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        const save = JSON.parse(readFileSync(join(assets.root, "saves", "slot12.sav"), "utf8"));
        assert.equal(save.metadata.mission, "earth.mis");
        await click(game, DEV_DONE);
        await click(game, pauseEntry(0));

        // This extra spawn must disappear even if a transition caches this newer world.
        await game.input.trigger("SpawnDebugMonster");
        await game.step({ frames: 1 });
        assert.equal((await game.entities.byTemplate(-397)).length, savedCount + 1);
        if (changeMission) {
          await game.transitionLevel("medsci1.mis");
          await game.waitFor(async () => (await game.info()).mission === "medsci1.mis", {
            timeoutMs: 60_000, description: "switch away from the saved mission",
          });
        }

        await game.input.trigger("TogglePauseMenu");
        await game.step({ frames: 5 });
        await click(game, pauseEntry(2));
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        await finishLoading(game, vr);
        const loaded = await game.info();
        assert.equal(loaded.mission, "earth.mis", "load must return to the saved mission");
        assert.equal(loaded.paused, false);
        assert.equal((await game.entities.byTemplate(-397)).length, savedCount,
          "load must restore saved spawns and remove post-save spawns");
      });
  }

  test(`pause save slots and load preserve the live mission (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      using assets = isolatedAssets();
      const click = vr ? vrClickCanvasPoint : clickCanvas;
      const launch = (mission: string, saveFile?: string) => GameServer.launch({
        mission,
        debugFlags: ["--defer-transitions", ...(vr ? ["--vr"] : []), ...(saveFile ? ["--save-file", saveFile] : [])],
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
        assert.equal(saved.metadata.format_version, 1);
        assert.equal(typeof saved.metadata.writer.version, "string");
        assert.match(saved.metadata.writer.commit, /^[0-9a-f]{40}$/);
        assert.equal(typeof saved.metadata.writer.dirty, "boolean");
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

        for (const failure of ["corrupt", "missing", "missing mission", "future format", "old schema"] as const) {
          await click(game, pauseEntry(2));
          await click(game, SLOT_ROW);
          if (failure === "corrupt") writeFileSync(savePath, "invalid save");
          else if (failure === "missing") rmSync(savePath);
          else if (failure === "future format") {
            const future = JSON.parse(goodSave.toString("utf8"));
            future.metadata.format_version = 999;
            writeFileSync(savePath, JSON.stringify(future));
          }
          else if (failure === "old schema") {
            const old = JSON.parse(goodSave.toString("utf8"));
            delete old.global_data.quest_info.mini_games;
            writeFileSync(savePath, JSON.stringify(old));
          }
          else {
            const unavailable = JSON.parse(goodSave.toString("utf8"));
            const mission = "missing-save-review.mis";
            unavailable.level_data[mission] = unavailable.level_data[unavailable.global_data.active_mission];
            unavailable.global_data.active_mission = mission;
            unavailable.metadata.mission = mission;
            writeFileSync(savePath, JSON.stringify(unavailable));
          }
          await click(game, DEV_ACTION);
          await finishLoading(game, vr);
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
        await finishLoading(game, vr);
        const loaded = await game.info();
        assert.equal(loaded.paused, false, "successful load resumes gameplay");
        assert.equal(loaded.player.hit_points, savedHitPoints);

        // Quickload uses the same deferred path; direct API loads stay synchronous.
        writeFileSync(join(assets.root, "saves", "save1.sav"), goodSave);
        await game.input.trigger("QuickLoad");
        await game.step({ frames: 1 });
        await finishLoading(game, vr);
        assert.equal((await game.info()).player.hit_points, savedHitPoints);

        // Quickload has no picker; its failure must reach the gameplay HUD instead.
        const incompatible = JSON.parse(goodSave.toString("utf8"));
        delete incompatible.global_data.quest_info.mini_games;
        writeFileSync(join(assets.root, "saves", "save1.sav"), JSON.stringify(incompatible));
        await game.input.trigger("QuickLoad");
        await game.step({ frames: 1 });
        await finishLoading(game, vr);
        assert.equal((await game.info()).mission, "medsci1.mis");
        assert.ok((await game.ui.state()).messages.includes(
          "This save is incompatible with the current version."));
      }

      // A second runtime proves the slot is read from disk across process lifetimes.
      {
        await using game = await launch("main_menu");
        await game.step({ frames: 5 });
        await click(game, [489.5, 126]);
        const sounds = (await game.audio.recent()).sounds;
        const lastSound = sounds.at(-1)?.sequence ?? 0;
        await click(game, [362, 63.5]);
        assert.equal((await game.info()).mission, "load_game");
        assert.equal((await game.audio.recent()).sounds.some(sound =>
          sound.sequence > lastSound && sound.sample === "sfx/mselect1.wav"), false,
          "an empty Load row must not play a selection sound");
        await click(game, SLOT_ROW);
        await click(game, DEV_ACTION);
        await finishLoading(game, vr);
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
