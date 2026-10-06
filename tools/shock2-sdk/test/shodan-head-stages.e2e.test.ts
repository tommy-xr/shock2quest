import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("SHODAN real rifle hits drive both authored screen stages across save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "shodan.mis" });
  await game.step({ frames: 60 });
  // Explicit isolated fixture: a visible position inside the shield perimeter.
  // Every HP change comes from the ordinary rifle, never a Damage/Slay request.
  await game.player.setStats({ strength: 4, endurance: 4, agility: 3, skills: { standard_weapons: 6 } });
  await game.player.spawnItem(-18);
  for (let i = 0; i < 4; i++) await game.player.spawnItem("Small Standard Clip");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 2 });
  await game.input.trigger("Reload");
  await game.step({ frames: 120 });
  const find = async (template: number) => (await game.entities.list({ limit: 2000 })).entities.find(e => e.template_id === template);
  const headHp = async () => {
    const head = await find(298); assert.ok(head);
    return Number((await game.entities.detail(head.id)).properties.find(p => p.name === "HitPoints")?.value);
  };
  const shootTo = async (threshold: number) => {
    for (let shot = 0; shot < 15 && await headHp() > threshold; shot++) {
      const head = await find(298); assert.ok(head);
      await game.player.teleport({ x: head.position[0], y: head.position[1] - 1.04, z: head.position[2] + 2 });
      await game.step({ frames: 1 });
      await game.player.aimAt(head.id, { visibility: "required", hitbox: "torso" });
      await game.input.set("right_hand.trigger_value", 1);
      await game.step({ frames: 2 });
      await game.input.set("right_hand.trigger_value", 0);
      await game.step({ frames: 5 });
      if (await headHp() <= threshold) break;
      await game.step({ frames: 25 });
    }
    assert.ok(await headHp() <= threshold && await headHp() > 0);
  };
  assert.ok(await find(821)); assert.ok(await find(822));
  await shootTo(82);
  await game.step({ frames: 30 });
  assert.ok(await find(821), "the authored one-second delay is not bypassed");
  await game.step({ frames: 60 });
  assert.equal(await find(821), undefined);
  assert.ok(await find(822));
  const save = `shodan_stage1_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  await shootTo(41);
  await game.step({ frames: 90 });
  assert.equal(await find(821), undefined);
  assert.equal(await find(822), undefined);
  assert.equal((await game.info()).player.life_state, "alive");
});
