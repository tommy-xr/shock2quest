import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`Rad Shield resists direct radiation without purging it and expires (${vr ? "VR" : "flat"})`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 5 });
    if (vr) {
      const [amp] = await game.entities.byTemplate(-247);
      assert.ok(amp);
      await aimVrHandAt(game, amp.position, .2, 0, 0);
      await aimVrHandAt(game, (await game.entities.detail(amp.id)).position, .2, 1, 0);
      await game.step({ frames: 3 });
    }
    const player = (await game.info()).player;
    await game.entities.sendMessage(player.entity_id!, { type: "Hazard", toxin: false, amount: 4 });
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.radiation_level, 4);
    await selectPsiPower(game, "Rad Shield");
    const psi = (await game.info()).player.psi_points!;
    await pullTrigger(game);
    await game.step({ frames: 2 });
    const active = (await game.info()).player;
    assert.equal(active.psi_points, psi - 2);
    assert.ok(active.active_psi_powers.includes("Rad Shield"));
    assert.equal(active.radiation_level, 4, "casting resistance does not cure stored radiation");
    await game.entities.sendMessage(player.entity_id!, { type: "Hazard", toxin: false, amount: 10 });
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.radiation_level, 9.5,
      "direct Radiate reactions interpret player Armor.Rad as 5 percent, not the room divisor");
    await game.step({ frames: 3000 });
    assert.ok(!(await game.info()).player.active_psi_powers.includes("Rad Shield"));
    await game.entities.sendMessage(player.entity_id!, { type: "Hazard", toxin: false, amount: 10 });
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.radiation_level, 10);
  });
}

test("Rad Shield divides authored room absorption by five through save/load and stops protecting on expiry", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  let unprotected: number;
  {
    await using baseline = await GameServer.launch({ mission: "eng1.mis" });
    await baseline.step({ frames: 2 });
    const room = (await baseline.entities.list()).entities.find(e => e.template_id === 226 && e.name === "Base Room");
    assert.ok(room);
    await baseline.player.teleport({ x: room.position[0], y: room.position[1] - .5, z: room.position[2] });
    await baseline.step({ frames: 1 });
    const before = (await baseline.info()).player.radiation_level;
    await baseline.step({ frames: 60 });
    unprotected = (await baseline.info()).player.radiation_level - before;
    assert.ok(unprotected > .4, `unprotected room absorption ${unprotected}`);
  }
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 5 });
  await selectPsiPower(game, "Rad Shield");
  await pullTrigger(game);
  await game.step({ frames: 2 });
  assert.ok((await game.info()).player.active_psi_powers.includes("Rad Shield"));
  await game.transitionLevel("eng1.mis");
  await game.step({ frames: 2 });
  const start = (await game.info()).player.position;
  const room = (await game.entities.list()).entities.find(e => e.template_id === 226 && e.name === "Base Room");
  assert.ok(room, "authored Engineering radiation room");
  const inside = { x: room.position[0], y: room.position[1] - .5, z: room.position[2] };
  const outside = { x: start[0], y: start[1], z: start[2] };
  async function exposure() {
    await game.player.teleport(inside);
    await game.step({ frames: 1 });
    const before = (await game.info()).player.radiation_level;
    await game.step({ frames: 60 });
    return (await game.info()).player.radiation_level - before;
  }
  assert.ok((await game.info()).player.active_psi_powers.includes("Rad Shield"),
    "cast protection survives a mission transition");
  const protectedAmount = await exposure();
  assert.ok(Math.abs(protectedAmount - unprotected / 5) < .025,
    `room absorption ${unprotected} -> ${protectedAmount}, expected fifth`);
  const savedLevel = (await game.info()).player.radiation_level;
  const save = `rad_shield_e2e_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  assert.ok((await game.info()).player.active_psi_powers.includes("Rad Shield"));
  assert.ok(Math.abs((await game.info()).player.radiation_level - savedLevel) < .025);
  const restoredAmount = await exposure();
  assert.ok(Math.abs(restoredAmount - unprotected / 5) < .025,
    `restored room absorption ${restoredAmount}`);
  await game.player.teleport(outside);
  await game.step({ frames: 3000 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Rad Shield"));
  const expiredAmount = await exposure();
  assert.ok(Math.abs(expiredAmount - unprotected) < .025,
    `expired room absorption ${expiredAmount}, expected ${unprotected}`);
});
