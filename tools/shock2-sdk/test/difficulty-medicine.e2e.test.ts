import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";

for (const difficulty of ["easy","normal"] as const) {
  test(`difficulty medicine: ${difficulty} uses authored patch and psi-hypo bonuses`, {
    skip:process.env.SHOCK2_E2E !== "1",timeout:180_000,
  },async () => {
    await using game = await GameServer.launch({mission:"medsci1.mis",difficulty});
    await game.step({frames:2});
    const initial = (await game.info()).player;
    await game.entities.sendMessage(initial.entity_id!,{type:"Damage",amount:25});
    await game.step({frames:1});
    const wounded = initial.max_hit_points! - 25;
    assert.equal((await game.info()).player.hit_points,wounded);
    const patch = await game.player.spawnItem("Med Patch");
    await game.entities.sendMessage(patch.entity_id,{type:"Frob"});
    await game.step({frames:7});
    assert.equal((await game.info()).player.hit_points,wounded+2,"first pulse remains two points");
    assert.ok(!(await game.player.inventory()).items.some(i=>i.entity_id===patch.entity_id));
    const save = `difficulty-medicine-${difficulty}-${Date.now()}`;
    await game.save(save);
    await game.load(save);
    await game.step({frames:720});
    assert.equal((await game.info()).player.hit_points,wounded+(difficulty === "easy" ? 15 : 10),"saved course retains its exact remaining budget");

    // Make room for the entire hypo amount, then let Earth's authored lesson
    // drain psi to five through its real entry tripwire.
    await game.player.setStats({psionic_ability:4});
    await game.transitionLevel("earth.mis");
    await crossEarthTrainingTripwire(game,320,325);
    assert.equal((await game.info()).player.psi_points,5);
    const hypo = await game.player.spawnItem("Psi Booster");
    await game.entities.sendMessage(hypo.entity_id,{type:"Frob"});
    await game.step({frames:2});
    assert.equal((await game.info()).player.psi_points,difficulty === "easy" ? 35 : 25);
    assert.ok(!(await game.player.inventory()).items.some(i=>i.entity_id===hypo.entity_id));
  });
}
