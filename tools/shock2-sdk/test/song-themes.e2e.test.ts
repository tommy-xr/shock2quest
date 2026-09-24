import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test("music markers supply quiet again after reloading the same mission", {
  skip: !e2eEnabled && "set SHOCK2_E2E=1 to run",
}, async () => {
  await using game = await GameServer.launch({
    mission: "medsci1.mis",
    rustLog: "debug_runtime=info,dark::audio::song_player=info",
  });
  for (let pass = 0; pass < 2; pass++) {
    if (pass > 0) {
      await game.input.trigger("DebugReloadLevel");
      await game.step({ frames: 1 });
    }
    // Discover the runtime ID/position afresh; mission object IDs are stable,
    // but runtime entity IDs change on each load.
    const { entities } = await game.entities.list({ filter: "music quiet turret1" });
    assert.equal(entities.length, 1);
    const [x, y, z] = entities[0].position;
    await game.player.teleport({ x, y, z });
    const before = game.logs().length;
    await game.waitFor(async () => {
      await game.step({ frames: 1 });
      return game.logs().slice(before).some(line =>
        line.includes("song transition") && line.includes('theme quiet'));
    }, { timeoutMs: 15000, description: "marker supplies theme quiet to song player" });
  }
});
