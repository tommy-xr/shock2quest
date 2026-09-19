import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test("horde preparation is silent and READY starts the first level's song", {
  skip: !e2eEnabled && "set SHOCK2_E2E=1 to run",
}, async () => {
  await using game = await GameServer.launch({
    mission: "earth_horde_test",
    rustLog: "debug_runtime=info,shock2vr::mission::mission_core=info,dark::audio::song_player=info",
  });
  await game.step({ frames: 2 });
  assert.ok(!game.logs().some(line => line.includes("song transition")), "preparation has no song clips");
  const { entities } = await game.entities.list({ filter: "Containment - Ready" });
  assert.equal(entities.length, 1);
  await game.entities.sendMessage(entities[0].id, { type: "Frob" });
  await game.step({ frames: 3 });
  await game.waitFor(async () => game.logs().some(line => line.includes("song transition")));
  const logs = game.logs();
  assert.ok(logs.some(line => line.includes("loading music for level: song08")));
  assert.ok(logs.some(line => line.includes("song transition") && line.includes("theme begin") && line.includes("08beg.wav")));
  assert.equal((await game.info()).mission, "earth_horde_test");
});
