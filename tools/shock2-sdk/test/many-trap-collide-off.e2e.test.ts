import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("many: real cluster destruction opens the sphincter to movement across save/load", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "many.mis" });
  const find = async (id: number) => (await game.entities.list({ limit: 2000 })).entities.find(e => e.template_id === id);
  const approach = async () => {
    await game.player.teleport({ x: 11.5, y: 2.8, z: 24 });
    await game.input.lookAtWorldPoint([11.5, 2.8, 40]);
    await game.step({ frames: 30 });
    await game.input.lookAtWorldPoint([11.5, 2.8, 40]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 45 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    return (await game.player.position()).z;
  };
  assert.ok(await approach() < 27, "closed gate must block normal locomotion");
  await game.player.setStats({ strength: 4, endurance: 4, agility: 3, skills: { standard_weapons: 6 } });
  await game.player.spawnItem(-18);
  await game.player.spawnItem("Small Standard Clip");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 2 });
  await game.input.trigger("Reload");
  await game.step({ frames: 120 });
  // Teleports stage each shot; aiming, firing, death, TriggerMulti and its
  // SwitchLink delivery use production gameplay, with no injected message.
  for (const [id, x, y, z] of [
    [118, -21.728067, -9.162045, -109.15292],
    [124, 43.320194, -11.416469, -135.06891],
  ]) {
    const cluster = await find(id);
    assert.ok(cluster, `cluster ${id} must still be alive`);
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 10 });
    await game.player.aimAt(cluster.id, { visibility: "required" });
    await game.input.set("right_hand.trigger_value", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger_value", 0);
    await game.step({ frames: 25 });
    assert.equal(await find(id), undefined, `rifle shot must destroy cluster ${id}`);
    if (id === 118) assert.ok(await approach() < 27, "one cluster must not open the two-input gate");
  }
  await game.step({ frames: 90 });
  const crossed = await approach();
  assert.ok(crossed > 29.5, `open gate must permit walking through; stopped at z=${crossed}`);
  // Save from a settled stance, not transient motion on the rising tube floor.
  await game.player.teleport({ x: 11.5, y: 2.8, z: 24 });
  await game.step({ frames: 60 });
  const save = `many_collide_off_${Date.now()}`;
  assert.equal((await game.save(save)).success, true);
  assert.equal((await game.load(save)).success, true);
  assert.ok(await find(969), "gate must remain present after load");
  const restored = await approach();
  assert.ok(restored > 29.5, `saved collision properties must preserve passage; stopped at z=${restored}`);
});
