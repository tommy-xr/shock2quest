import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

for (const difficulty of ["easy", "normal", "hard", "impossible"] as const) {
  test(`Psi Sword retains the amp, costs four PSI and expires on ${difficulty}`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_psi", difficulty });
      await game.step({ frames: 10 });
      await selectPsiPower(game, "Psi Sword");
      const before = (await game.info()).player;
      assert.ok(before.wielded_entity_id !== null);
      await pullTrigger(game);
      const active = (await game.info()).player;
      assert.equal(active.wielded_entity_id, before.wielded_entity_id, "the original amp stays wielded");
      assert.equal(active.psi_points, before.psi_points! - 4);
      assert.ok(active.active_psi_powers.includes("Psi Sword"));
      const [hybrid] = await game.entities.byTemplate(-397);
      assert.ok(hybrid);
      const target = await game.entities.detail(hybrid.id);
      const hp = Number(target.properties.find(p => p.name === "HitPoints")!.value);
      const [x,y,z] = target.position;
      await game.player.teleport({ x: x+1, y: y+0.5, z });
      await game.step({ frames: 10 });
      await game.player.aimAt(hybrid, { hitbox: "torso", visibility: "required" });
      await pullTrigger(game);
      assert.equal((await game.info()).player.psi_points, active.psi_points, "swinging does not recast or spend PSI");
      await game.step({ frames: 65 });
      const survivor = (await game.entities.byTemplate(-397)).find(e => e.id === hybrid.id);
      const remaining = survivor ? Number((await game.entities.detail(hybrid.id)).properties.find(p => p.name === "HitPoints")!.value) : 0;
      assert.ok(hp - remaining > 6, `authored sword damage exceeds wrench: ${hp} -> ${remaining}`);
      await game.player.teleport({ x: 0, y: 2, z: 0 });
      await game.step({ frames: 60 * 60 });
      const expired = (await game.info()).player;
      assert.ok(!expired.active_psi_powers.includes("Psi Sword"));
      assert.equal(expired.wielded_entity_id, before.wielded_entity_id, "expiry leaves the same amp");
      await pullTrigger(game);
      await game.step({ frames: 5 });
      assert.ok((await game.info()).player.active_psi_powers.includes("Psi Sword"));
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 10 });
      assert.ok(!(await game.info()).player.active_psi_powers.includes("Psi Sword"), "holstering cancels the blade");
      assert.equal((await game.entities.byTemplate(-247)).length, 1, "no duplicate amp was created");
    });
}

test("additive blade survives a mission transition and save/load without losing its expiry", {
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
  assert.notEqual((await game.entities.detail(loaded.wielded_entity_id!)).name, "PsiSword");
  const amps = await game.entities.byTemplate(-247);
  assert.equal(amps.length, 1, "transition and load preserve exactly one original amp");
  for (let i = 0; i < 15; i++) await game.step({ frames: 300 });
  const expired = (await game.info()).player;
  assert.deepEqual(expired.active_psi_powers, []);
  assert.equal(expired.wielded_entity_id, amps[0].id, "expiry keeps the remapped amp");
  assert.equal((await game.entities.byTemplate(-2291)).length, 0);
});


for (const invisible of [false, true]) {
  test(`VR sword physical swing ${invisible ? "ends invisibility" : "survives zero-time input updates"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const [amp] = await game.entities.byTemplate(-247);
    assert.ok(amp);
    await aimVrHandAt(game, amp.position, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
    await game.input.set("head.look", [0,0]);
    await aimVrHandAt(game, [-3,1.6,-1], 1, 1, 0, { lookAtTarget: false });
    await game.step({ frames: 13 });
    if (invisible) {
      await selectPsiPower(game, "Inviso");
      await pullTrigger(game);
      assert.ok((await game.info()).player.active_psi_powers.includes("Inviso"));
    }
    await selectPsiPower(game, "Psi Sword");
    await pullTrigger(game);
    await game.step({ frames: 6 });
    const target = (await game.entities.list({ filter: "OG-Pipe" })).entities.sort((a,b) => a.distance-b.distance)[0];
    assert.ok(target);
    await game.player.teleport({ x:-7.8, y:1.2, z:-1.4 });
    await game.step({ frames: 6 });
    await game.input.lookAtWorldPoint(target.position, { eyeHeight: (await game.info()).player.camera_offset[1] });
    const hp = async () => Number((await game.entities.detail(target.id)).properties.find(p => p.name === "HitPoints")!.value);
    const before = await hp();
    assert.ok(before > 0);
    if (invisible) assert.ok((await game.info()).player.active_psi_powers.includes("Inviso"), "preparing the swing keeps stealth");
    // Each helper call sends zero-time input patches, then advances three frames.
    // Those patches must not consume the tracked movement before the swing tick.
    for (let i=0; i<24 && await hp()>0; i++) {
      await aimVrHandAt(game, [-9,1.6,-1.2+i*0.1], 0.2, 1, 0, { lookAtTarget: false });
    }
    assert.equal(await hp(), 0, "a physical sweep hits without pressing the trigger");
    if (invisible) assert.ok(!(await game.info()).player.active_psi_powers.includes("Inviso"), "a qualifying VR strike breaks stealth");
    assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
  });

}
