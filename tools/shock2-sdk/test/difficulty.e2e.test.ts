import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  test(`difficulty: ${difficulty} is retained across save/load and deck transitions`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", difficulty });
      await game.step({ frames: 2 });
      const initial = (await game.info()).player;
      assert.equal(initial.difficulty, difficulty);
      const pools = { easy: [55, 26], normal: [35, 15], hard: [27, 11], impossible: [10, 6] }[difficulty];
      assert.equal(initial.hit_points, pools[0]);
      assert.equal(initial.max_hit_points, pools[0]);
      assert.equal(initial.psi_points, pools[1]);
      assert.equal(initial.max_psi_points, pools[1]);
      await game.entities.sendMessage(initial.entity_id!, { type: "Damage", amount: 2 });
      await game.step({ frames: 1 });
      const wounded = (await game.info()).player.hit_points;
      assert.equal(wounded, pools[0] - 2);
      await assert.rejects(game.quests.set("Difficulty", "complete"), /fixed/);
      const save = `difficulty-${difficulty}-${Date.now()}`;
      await game.save(save);
      await game.transitionLevel("medsci2.mis");
      assert.equal((await game.info()).player.difficulty, difficulty);
      await game.load(save);
      assert.equal((await game.info()).player.difficulty, difficulty);
      assert.equal((await game.info()).mission, "medsci1.mis");
      assert.equal((await game.info()).player.hit_points, wounded, "load does not refill the pool");
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

test("difficulty: stat and psi-tier upgrades adjust current pools by the maximum delta", {
  skip: !enabled, timeout: 120_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", difficulty: "normal" });
  await game.step({ frames: 2 });
  await game.entities.sendMessage((await game.info()).player.entity_id!, { type: "Damage", amount: 2 });
  await game.step({ frames: 1 });
  await game.player.setStats({ endurance: 2, psionic_ability: 2, psi_tier: 2 });
  await game.step({ frames: 1 });
  const player = (await game.info()).player;
  assert.equal(player.max_hit_points, 40);
  assert.equal(player.hit_points, 38);
  assert.equal(player.max_psi_points, 31);
  assert.equal(player.psi_points, 31);
  await game.player.setStats({ cyber_modules: 100 });
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.hit_points, 38, "unrelated provisioning cannot heal");
});
