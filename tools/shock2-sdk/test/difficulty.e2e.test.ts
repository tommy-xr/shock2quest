import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  test(`difficulty: ${difficulty} is retained across save/load and deck transitions`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", difficulty });
      await game.step({ frames: 2 });
      assert.equal((await game.info()).player.difficulty, difficulty);
      await assert.rejects(game.quests.set("Difficulty", "complete"), /fixed/);
      const save = `difficulty-${difficulty}-${Date.now()}`;
      await game.save(save);
      await game.transitionLevel("medsci2.mis");
      assert.equal((await game.info()).player.difficulty, difficulty);
      await game.load(save);
      assert.equal((await game.info()).player.difficulty, difficulty);
      assert.equal((await game.info()).mission, "medsci1.mis");
    });
}

test("difficulty: loading another campaign overrides the launch choice", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  const save = `difficulty-cross-campaign-${Date.now()}`;
  {
    await using game = await GameServer.launch({ mission: "medsci1.mis", difficulty: "impossible" });
    await game.step({ frames: 2 });
    await game.save(save);
  }
  await using game = await GameServer.launch({ mission: "medsci1.mis", difficulty: "easy" });
  assert.equal((await game.info()).player.difficulty, "easy");
  await game.load(save);
  assert.equal((await game.info()).player.difficulty, "impossible");
});
