import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("Survive alarms follow waves and stay off through rest and reload", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth_horde_test" });
  await game.devParams.set("cheat", 1);
  await game.devParams.set("horde_tech_enabled", 0);
  await game.step({ frames: 2 });
  const light = async () => {
    const [entity] = await game.entities.byTemplate(87);
    assert.ok(entity);
    return (await game.entities.detail(entity.id)).properties.find(p => p.name === "AnimLightIntensity")?.value;
  };
  const beeps = async () => (await game.audio.recent()).sounds.filter(s => s.sample === "lobeep1").length;
  for (let i = 0; i < 24; i++) {
    await game.step({ frames: 3 });
    assert.equal(await light(), "0.000", "preparation keeps the red alarm off");
  }
  assert.equal(await beeps(), 0);
  const [ready] = (await game.entities.list({ filter: "Containment - Ready" })).entities;
  assert.ok(ready);
  await game.entities.sendMessage(ready.id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal(await light(), "1.000");
  assert.ok(await beeps() > 0, "starting a wave plays the authored alarm");
  await game.step({ frames: 12 });
  assert.equal(await light(), "0.000", "active waves retain authored blink timing");
  const markers = (await game.entities.list({ filter: "Containment spawn" })).entities;
  for (let i = 0; i < 18; i++) {
    const spawned = (await Promise.all(markers.map(m => game.entities.detail(m.id))))
      .flatMap(m => m.outgoing_links).filter(l => l.link_type === "Spawned");
    for (const link of spawned) {
      await game.entities.sendMessage(link.target_id, { type: "Damage", amount: 10000 });
    }
    await game.step({ frames: 60 });
  }
  assert.equal(await light(), "0.000");
  const slot = `survive_alarms_${Date.now()}`;
  assert.equal((await game.save(slot)).success, true);
  assert.equal((await game.load(slot)).success, true);
  await game.step({ frames: 2 });
  const count = await beeps();
  for (let i = 0; i < 24; i++) {
    await game.step({ frames: 3 });
    assert.equal(await light(), "0.000", "rest stays dark after loading");
  }
  assert.equal(await beeps(), count, "rest emits no alarm sounds");
  const [restoredReady] = (await game.entities.list({ filter: "Containment - Ready" })).entities;
  await game.entities.sendMessage(restoredReady.id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal(await light(), "1.000", "the next wave restarts the alarms");
});
