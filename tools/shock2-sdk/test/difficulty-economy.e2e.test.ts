import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { physicallyOpenEarthReplicator } from "./helpers/earth-replicator.js";
import { clickUiElement } from "./helpers/ui.js";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  test(`difficulty economy: ${difficulty} quotes match trainer and replicator charges`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
  }, async () => {
    const trainerPrice = {easy:2, normal:3, hard:4, impossible:5}[difficulty];
    const replicatorPrice = {easy:2, normal:3, hard:3, impossible:6}[difficulty];
    await using game = await GameServer.launch({mission:"medsci1.mis",difficulty});
    await game.player.setStats({cyber_modules:100});
    const [trainer] = await game.entities.byTemplate(1352);
    assert.ok(trainer);
    const [x,y,z] = (await game.entities.detail(trainer.id)).position;
    await teleportVerified(game,{x,y:y+0.5,z:z+1.2});
    await game.step({frames:30});
    await game.entities.sendMessage(trainer.id,{type:"Frob"});
    await game.step({frames:5});
    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel);
    assert.ok(panel.elements.some(e => e.text?.includes(`lvl 1 > 2: ${trainerPrice} cm`)));
    const endurance = panel.elements.find(e => e.kind === "button" && e.label === "Endurance");
    assert.ok(endurance);
    await clickUiElement(game,endurance);
    assert.equal((await game.info()).player.stats?.cyber_modules,100-trainerPrice);
    await game.transitionLevel("earth.mis");
    const [nanites] = await game.entities.byTemplate(257);
    assert.ok(nanites);
    await game.entities.sendMessage(nanites.id,{type:"Frob"});
    await game.step({frames:5});
    const balance = (await game.info()).player.stats!.nanites;
    assert.equal(balance,250,"funded from authored nanite pile");
    const [replicator] = await game.entities.byTemplate(262);
    assert.ok(replicator);
    await physicallyOpenEarthReplicator(game,replicator);
    const inventory = (await game.ui.state()).active_panel;
    assert.ok(inventory);
    assert.ok(inventory.elements.some(e => e.text === String(replicatorPrice).padStart(3,"0")));
    const chips = inventory.elements.find(e => e.kind === "button" && e.label === "buy:chips");
    assert.ok(chips);
    const before = (await game.entities.byTemplate(-92)).length;
    await clickUiElement(game,chips);
    assert.equal((await game.info()).player.stats?.nanites,balance-replicatorPrice);
    assert.equal((await game.entities.byTemplate(-92)).length,before+1);
  });
}
