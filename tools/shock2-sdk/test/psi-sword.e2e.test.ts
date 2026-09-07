import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

test("Psi Sword conjures a blade and restores the amp on expiry", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  await game.player.setStats({ psionic_ability: 6 });
  const before = (await game.info()).player;
  await selectPsiPower(game, "Psi Sword");
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const cast = (await game.info()).player;
  assert.equal(cast.psi_points, before.psi_points! - 4);
  assert.notEqual(cast.wielded_entity_id, before.wielded_entity_id,
    "casting Psi Sword must replace the amp with its conjured blade");
  const blade = cast.wielded_entity_id!;
  assert.equal((await game.entities.detail(blade)).name, "PsiSword");
  assert.deepEqual(cast.active_psi_powers, ["Psi Sword"]);
  const [hybrid] = await game.entities.byTemplate(-397);
  assert.ok(hybrid, "the psi pen has an authored hybrid target");
  const target = await game.entities.detail(hybrid.id);
  const hp = Number(target.properties.find((p) => p.name === "HitPoints")!.value);
  assert.ok(hp > 6, "target must survive an ordinary six-point wrench hit");
  const [x, y, z] = target.position;
  await game.player.teleport({ x: x + 1, y: y + 0.5, z });
  await game.step({ frames: 10 });
  await game.player.aimAt(hybrid, { hitbox: "torso", visibility: "required" });
  await pullTrigger(game);
  await game.step({ frames: 65 });
  const survivor = (await game.entities.byTemplate(-397)).find((e) => e.id === hybrid.id);
  const afterHp = survivor
    ? Number((await game.entities.detail(hybrid.id)).properties.find((p) => p.name === "HitPoints")!.value)
    : 0;
  assert.ok(hp - afterHp > 6, `psi blade should hit harder than a wrench: ${hp} -> ${afterHp}`);
  await game.player.teleport({ x: 0, y: 2, z: 0 });
  await game.step({ frames: 4260 });
  assert.equal((await game.info()).player.wielded_entity_id, before.wielded_entity_id);
  assert.deepEqual((await game.info()).player.active_psi_powers, []);
  assert.ok(!(await game.entities.byTemplate(-2291)).some((e) => e.id === blade), "expired blade is destroyed");
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const secondBlade = (await game.info()).player.wielded_entity_id;
  assert.notEqual(secondBlade, before.wielded_entity_id);
  await game.input.trigger("EquipPsiAmp");
  await game.step({ frames: 10 });
  assert.equal((await game.info()).player.wielded_entity_id, before.wielded_entity_id);
  assert.deepEqual((await game.info()).player.active_psi_powers, []);
  assert.ok(!(await game.entities.byTemplate(-2291)).some((e) => e.id === secondBlade), "holstering cancels and destroys the summon");
});

test("summoned blade survives a mission transition and save/load without losing its expiry", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  await selectPsiPower(game, "Psi Sword");
  await pullTrigger(game);
  await game.step({ frames: 10 });
  await game.transitionLevel("earth.mis");
  await game.step({ frames: 10 });
  const crossed = (await game.info()).player;
  assert.deepEqual(crossed.active_psi_powers, ["Psi Sword"], JSON.stringify(crossed));
  const saveName = `e2e_psi_sword_${Date.now()}`;
  await game.save(saveName);
  await game.load(saveName);
  await game.step({ frames: 10 });
  const loaded = (await game.info()).player;
  assert.deepEqual(loaded.active_psi_powers, ["Psi Sword"]);
  assert.equal((await game.entities.detail(loaded.wielded_entity_id!)).name, "PsiSword");
  const amps = await game.entities.byTemplate(-247);
  assert.equal(amps.length, 1, "transition and load preserve exactly one original amp");
  for (let i = 0; i < 15; i++) await game.step({ frames: 300 });
  const expired = (await game.info()).player;
  assert.deepEqual(expired.active_psi_powers, []);
  assert.equal(expired.wielded_entity_id, amps[0].id, "expiry restores the remapped amp");
  assert.equal((await game.entities.byTemplate(-2291)).length, 0);
});
