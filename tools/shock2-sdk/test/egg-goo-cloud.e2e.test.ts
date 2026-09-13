import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("goo pod's cloud poisons immediately before its globs, then expires", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  await game.player.teleport({ x: -6, y: 1, z: -6 });
  await game.step({ frames: 3 });
  assert.equal((await game.entities.byTemplate(-1557)).length, 0, "no glob may have delivered the toxin yet");
  assert.equal((await game.entities.byTemplate(-438)).length, 1);
  const exposure = (await game.info()).player.toxin_level;
  // The shell occludes part of the shared radius effect's body-ray samples.
  assert.ok(exposure > 0 && exposure <= 2, "cloud delivers authored Venom through the cover-aware radius path");
  const hp = (await game.info()).player.hit_points!;
  await game.player.teleport({ x: 0, y: 1, z: -6 });
  await game.step({ frames: 90 });
  assert.equal((await game.entities.byTemplate(-438)).length, 0, "spent particle cloud must be removed");
  assert.equal((await game.info()).player.toxin_level, exposure, "leaving the burst does not cure poison");
  await game.step({ frames: 600 });
  assert.ok((await game.info()).player.hit_points! < hp, "natural Venom exposure must feed toxin damage ticks");
});

test("goo cloud does not poison distant players or pulse again for a late arrival", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_annelid" });
  await game.step({ frames: 1 });
  const [pod] = await game.entities.byTemplate(-1476);
  assert.ok(pod);
  await game.entities.sendMessage(pod.id, { type: "TurnOn" });
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.toxin_level, 0);
  await game.player.teleport({ x: -6, y: 1, z: -6 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.toxin_level, 0, "one-shot source must not fire again each frame");
});
