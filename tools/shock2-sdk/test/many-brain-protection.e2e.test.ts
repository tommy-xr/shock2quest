import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

test("many: real defender deaths release brain protection across full save/load", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "many.mis" });
  // Explicit regression fixture staging; every impact, death and SwitchLink
  // delivery below uses production weapons, never injected script messages.
  await game.player.setStats({ strength: 4, endurance: 4, agility: 3, skills: { standard_weapons: 6 } });
  await game.player.spawnItem(-18);
  await game.player.spawnItem("Small Standard Clip");
  await game.input.trigger("EquipAssaultRifle");
  await game.step({ frames: 2 });
  await game.input.trigger("Reload");
  await game.step({ frames: 120 });
  const discover = async () => {
    const all = (await game.entities.list({ limit: 2000 })).entities;
    const brain = all.find(e => e.template_id === 253);
    assert.ok(brain);
    return { brain, stars: all.filter(e => [845, 840, 594].includes(e.template_id)) };
  };
  const hp = async (id: number) => Number((await game.entities.detail(id)).properties.find(p => p.name === "HitPoints")?.value);
  const pose = async (position: number[]) => {
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.player.teleport({ x: position[0], y: position[1], z: position[2] });
    await game.step({ frames: 1 });
  };
  const shoot = async (id: number) => {
    let visible = false;
    // A defender can orbit behind the central base. Wait for an authored
    // opening instead of accepting an occluded aim or injecting damage.
    for (let orbit = 0; orbit < 12 && !visible; orbit++) {
      for (const position of [[293.75504, 13.574003, 150.33034], [313, 13.574003, 150], [304, 13.574003, 132], [304, 13.574003, 155]]) {
        await pose(position);
        try {
          await game.player.aimAt(id, { visibility: "required" });
          visible = true;
          break;
        } catch { /* Try the next declared staging view. */ }
      }
      if (!visible) await game.step({ frames: 60 });
    }
    assert.ok(visible, "authored target must have a clear real shot");
    await game.input.set("right_hand.trigger_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger_value", 0);
  };
  assert.equal((await discover()).stars.length, 3);
  for (let remaining = 3; remaining >= 0; remaining--) {
    await pose([302.6862, 6.941723, 150.71123]);
    await game.step({ frames: 60 });
    const save = `many_brain_${remaining}_${Date.now()}`;
    assert.equal((await game.save(save)).success, true);
    assert.equal((await game.load(save)).success, true);
    const state = await discover();
    assert.equal(state.stars.length, remaining);
    const before = await hp(state.brain.id);
    await shoot(state.brain.id);
    const shields = (await game.entities.list({ filter: "ball shield" })).entities;
    if (remaining > 0) {
      assert.equal(await hp(state.brain.id), before, "Abort receptrons must absorb the real hit after load");
      assert.equal(shields.length, remaining, "each surviving defender must flash on impact");
    } else {
      assert.ok(await hp(state.brain.id) < before, "the last defender's death must restore vulnerability");
      assert.equal(shields.length, 0);
    }
    await game.step({ frames: 60 });
    assert.equal((await game.entities.list({ filter: "ball shield" })).entities.length, 0, "authored deletion must expire the flash");
    assert.equal((await game.entities.list({ filter: "ballhaze" })).entities.length, 0, "shield children must not leak");
    if (remaining === 0) break;
    const template = state.stars[0].template_id;
    for (let shots = 0; shots < 6; shots++) {
      const star = (await discover()).stars.find(e => e.template_id === template);
      if (!star) break;
      await shoot(star.id);
      await game.step({ frames: 30 });
    }
    assert.equal((await discover()).stars.length, remaining - 1, "real shots must destroy exactly one defender");
  }
});
