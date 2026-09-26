import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const difficulty of ["easy","normal","hard","impossible"] as const) {
  test(`difficulty placement: ${difficulty} honors authored masks and survives reload`, {
    skip:process.env.SHOCK2_E2E !== "1", timeout:180_000,
  },async () => {
    await using game = await GameServer.launch({mission:"medsci1.mis",difficulty});
    const count = async (id:number) => (await game.entities.byTemplate(id)).length;
    assert.equal(await count(1363),difficulty === "easy" ? 1 : 0,"Easy-only world pistol");
    assert.equal(await count(2060),difficulty === "impossible" ? 0 : 1,"corpse pistol is absent on Impossible");
    const save = `difficulty-placement-${difficulty}-${Date.now()}`;
    await game.save(save);
    await game.transitionLevel("ops4.mis");
    for (const id of [317,319]) {
      assert.equal(await count(id),difficulty === "hard" || difficulty === "impossible" ? 1 : 0,"Hard/Impossible grenade hybrid");
    }
    await game.load(save);
    assert.equal(await count(1363),difficulty === "easy" ? 1 : 0);
    assert.equal(await count(2060),difficulty === "impossible" ? 0 : 1);
  });
}
