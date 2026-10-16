import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

test(
  "a traveling shot saved mid-flight resumes on load and hits once",
  {
    skip: process.env.SHOCK2_E2E !== "1",
    timeout: 240_000,
  },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.devParams.set("cheat", 1);
    await game.player.setStats({ skills: { standard_weapons: 3 } });
    await game.player.spawnItem("Pistol", { hand: "right" });
    await game.player.spawnItem("Standard Clip");
    await game.devParams.set("world_time_scale", 0.5);
    await game.devParams.set("slow_time_shot_speed", 0.25);
    await game.step({ frames: 60 });

    const bullet = async () =>
      (await game.entities.list({ filter: "Standard Bullet", limit: 5 }))
        .entities[0];
    const spangs = async () =>
      (await game.entities.list({ filter: "*Spang*", limit: 50 })).entities
        .length;
    // Step until the shot is gone: its path, and the spangs it left.
    const flight = async () => {
      const before = await spangs();
      const path: number[][] = [];
      for (let frame = 0; frame < 200; frame++) {
        await game.step({ frames: 1 });
        const shot = await bullet();
        if (!shot) {
          return { path, spangs: (await spangs()) - before };
        }
        path.push(shot.position);
      }
      assert.fail("the shot never landed");
    };

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 20 });
    assert.ok(await bullet(), "the slowed shot is still in flight");

    const save = `slow_shot_${Date.now()}`;
    assert.equal((await game.save(save)).success, true);
    const uninterrupted = await flight();
    assert.equal((await game.load(save)).success, true);
    const reloaded = await flight();

    assert.ok(uninterrupted.path.length > 1);
    assert.deepEqual(
      reloaded.path,
      uninterrupted.path,
      "same flight after load",
    );
    assert.equal(reloaded.spangs, 1, "the reloaded shot lands once");
    assert.equal(uninterrupted.spangs, 1);
  },
);
