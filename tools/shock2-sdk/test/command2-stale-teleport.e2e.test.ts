import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("command2: teleport relays move living targets and ignore destroyed boss targets", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "command2.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 1 });
  const entities = (await game.entities.list({ limit: 3000 })).entities;
  const authored = (template: number) => {
    const entity = entities.find(e => e.template_id === template);
    assert.ok(entity, `authored Command2 object ${template}`);
    return entity;
  };
  const brain = authored(189), body = authored(161);
  const brainTrap = authored(190), bodyTrap = authored(168);
  const live = async (template: number) => (await game.entities.list({ limit: 3000 })).entities.find(e => e.template_id === template);

  // Explicit isolated script fixture: exercise the production teleport effect
  // and death queue without requiring the full campaign or a private save.
  for (const [target, trap] of [[brain, brainTrap], [body, bodyTrap]]) {
    assert.ok(Math.hypot(...target.position.map((v, i) => v - trap.position[i])) > 1);
    await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    await game.step({ frames: 1 });
    const moved = await live(target.template_id);
    assert.ok(moved, "living target remains alive");
    assert.ok(Math.hypot(...moved.position.map((v, i) => v - trap.position[i])) < 0.2,
      `living target must reach teleport trap, got ${moved.position}`);
  }

  // BrainDead makes the projection mortal; both entities must be destroyed in
  // this same runtime. A save/load drops dead references and hides the defect.
  for (const target of [brain, body]) {
    await game.entities.sendMessage(target.id, { type: "Damage", amount: 10_000 });
    await game.step({ frames: 5 });
    assert.equal(await live(target.template_id), undefined, "lethal damage destroys the original target");
  }
  for (let relay = 0; relay < 2; relay++) {
    for (const trap of [brainTrap, bodyTrap]) {
      await game.entities.sendMessage(trap.id, { type: "TurnOn" });
    }
    await game.step({ frames: 5 });
    assert.equal((await game.info()).mission, "command2.mis");
    for (const target of [brain, body]) {
      assert.equal(await live(target.template_id), undefined, "stale teleport must never resurrect a target");
      // Debug IDs omit the ECS generation and can be reused by death FX.
      // The stable authored template, not that recycled index, proves absence.
    }
  }
});
