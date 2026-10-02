import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("completed organ research boosts matching enemies, including limb hits and saved games", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee" });
  await game.step({ frames: 30 });
  await game.player.setStats({ skills: { research: 6 } });

  const hp = async (id: number) => Number((await game.entities.detail(id)).properties.find(p => p.name === "HitPoints")?.value);
  const hit = async (id: number, expected: number, joint?: number) => {
    const before = await hp(id);
    const detail = await game.entities.detail(id);
    const aim = detail.aim_points?.find(p => p.joint_id === joint);
    if (joint !== undefined) assert.ok(aim, "the target has the requested limb");
    await game.entities.sendMessage(aim?.proxy_entity_id ?? id, {
      type: "Damage", amount: 4, point: aim?.position ?? detail.position, direction: [0, 0, 1],
    });
    await game.step({ frames: 3 });
    assert.equal(before - await hp(id), expected, "research and hit-location bonuses apply exactly once");
  };
  const hybrids = await game.entities.byTemplate(-397);
  assert.ok(hybrids.length >= 3);
  await hit(hybrids[0].id, 4);
  const organ = await game.player.spawnItem(-1095);
  await game.entities.sendMessage(organ.entity_id, { type: "Frob" });
  await game.step({ frames: 1 });
  await hit(hybrids[0].id, 4); // started research is not enough
  await game.step({ frames: 300 });
  assert.equal((await game.entities.detail(organ.entity_id)).properties.find(p => p.name === "ObjectState")?.value, "Normal");
  await hit(hybrids[1].id, 5); // retail organ multiplier is 1.25
  await hit(hybrids[2].id, 6, 9); // head: 4 * 1.25 * 1.25, rounded once

  const [spider] = await game.entities.byTemplate(-1439);
  assert.ok(spider, "the melee lab also has an unrelated arachnid");
  await hit(spider.id, 4);

  // Generated debug scenes intentionally cannot be saved. Carry the completed
  // research into a real mission, then round-trip it with the production save.
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 30 });
  const missionHybrids = (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities;
  const survivor = (await Promise.all(missionHybrids.map(async e => ({ ...e, hp: await hp(e.id) }))))
    .find(e => e.hp > 10);
  assert.ok(survivor, "Med/Sci has a surviving hybrid");
  await hit(survivor.id, 5);
  const saveName = `research_damage_${Date.now()}`;
  assert.equal((await game.save(saveName)).success, true);
  assert.equal((await game.load(saveName)).success, true);
  await game.step({ frames: 3 });
  const [restored] = await game.entities.byTemplate(survivor.template_id);
  assert.ok(restored, "the same mission hybrid is rediscovered after load");
  await hit(restored.id, 5);
});
