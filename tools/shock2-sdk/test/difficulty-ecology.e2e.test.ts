import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const difficulty of ["easy","normal"] as const) {
  test(`difficulty ecology: ${difficulty} follows authored alarm spawn timing`, {
    skip:process.env.SHOCK2_E2E !== "1",timeout:180_000,
  },async () => {
    await using game = await GameServer.launch({mission:"medsci1.mis",difficulty});
    const [camera] = await game.entities.byTemplate(77);
    assert.ok(camera);
    // Camera77 -> Diff ecology78 -> generator81. Its authored alert profile
    // is min/max2, period10s, random0; actual spawned OG-Pipe instances carry
    // archetype -397 while placed objects have positive mission identities.
    const count = async () => (await game.entities.byTemplate(-397)).length;
    assert.equal(await count(),0);
    await game.entities.sendMessage(camera.id,{type:"SetAlertness",level:"High"});
    // Sample beyond the timer boundary: initialization and queued spawning
    // can put the visible actor a few frames after the nominal deadline.
    await game.step({frames:660});
    assert.equal(await count(),difficulty === "easy" ? 0 : 1);
    if (difficulty === "easy") {
      const save = `difficulty-ecology-${Date.now()}`;
      await game.save(save);
      await game.load(save);
      assert.equal(await count(),0);
    }
    await game.step({frames:600});
    assert.equal(await count(),difficulty === "easy" ? 1 : 2);
    await game.step({frames:600});
    assert.equal(await count(),difficulty === "easy" ? 1 : 2,"population ceiling remains stable");
  });
}
