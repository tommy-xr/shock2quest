import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const brainDeath of ["living", "hidden"] as const) {
  test(`many: Overlord returns after saved temporary death; BrainDead while ${brainDeath} makes it mortal`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "many.mis" });
    await game.step({ frames: 60 });
    const safe = await game.player.position();
    // Explicit isolated fixture staging. All damage and death messages below
    // originate from production rifle shots; no health/message injection.
    await game.player.setStats({ strength: 4, endurance: 4, agility: 3, skills: { standard_weapons: 6 } });
    await game.player.spawnItem(-18);
    for (let i = 0; i < 5; i++) await game.player.spawnItem("Small Standard Clip");
    await game.input.trigger("EquipAssaultRifle");
    await game.step({ frames: 2 });
    const find = async (template: number) => (await game.entities.list({ limit: 2000 })).entities.find(e => e.template_id === template);
    const hp = async (id: number) => Number((await game.entities.detail(id)).properties.find(p => p.name === "HitPoints")?.value);
    const retreat = async () => {
      await game.input.set("right_hand.thumbstick", [0, 0]);
      await game.player.teleport(safe);
    };
    const kill = async (template: number) => {
      await retreat();
      await game.input.trigger("Reload");
      await game.step({ frames: 120 });
      let coveredFrames = 0;
      for (let shot = 0; shot < 30; shot++) {
        const target = await find(template);
        if (!target || await hp(target.id) <= 0) return;
        let aimed = false;
        const rejected: string[] = [];
        // The linked brain occupies a narrow side alcove. Stay close to its
        // flying projection and survey all three axes: one height can be fully
        // occluded by the boss base or orbiting balls as it crosses the arena.
        const offsets = template === 728
          ? [[3.103, 1.68, -3.59], [-3, 0, 0], [3, 0, 0], [0, 0, -3], [0, 0, 3], [-5, 0, -3], [5, 0, -3]]
          : [...[-4, 4].flatMap(x => [-4, 4].flatMap(y => [-4, 4].map(z => [x, y, z]))),
            [-4, 0, 0], [4, 0, 0], [0, -4, 0], [0, 4, 0], [0, 0, -4], [0, 0, 4]];
        for (const [x, y, z] of offsets) {
          await game.player.teleport({ x: target.position[0] + x, y: target.position[1] + y, z: target.position[2] + z });
          await game.step({ frames: 1 });
          try { await game.player.aimAt(target.id, { visibility: "required", hitbox: "torso" }); aimed = true; break; }
          catch (error) { rejected.push(String(error)); }
        }
        if (!aimed && template === 730 && coveredFrames < 600) {
          // The flying projection can pass behind the solid boss base. Let it
          // emerge before firing; failed visibility probes never count as shots.
          await retreat();
          await game.step({ frames: 30 });
          coveredFrames += 30;
          shot--;
          continue;
        }
        assert.ok(aimed, `real shot requires visible authored target ${template}: ${rejected.join("; ")}`);
        await game.input.set("right_hand.trigger_value", 1);
        await game.step({ frames: 2 });
        await game.input.set("right_hand.trigger_value", 0);
        const after = await find(template);
        if (!after || await hp(after.id) <= 0) return;
        await game.step({ frames: 25 });
        if (shot === 12) { await game.input.trigger("Reload"); await game.step({ frames: 120 }); }
      }
      assert.fail("bounded actual rifle sequence must be lethal");
    };
    const original = await find(730);
    assert.ok(original);
    await kill(730);
    assert.equal((await find(730))?.id, original.id, "temporary death retains the original entity");
    await retreat();
    await game.step({ frames: 120 });
    assert.equal((await game.entities.detail(original.id)).has_refs, false);
    assert.equal((await game.physics.bodies({ entityId: original.id })).bodies.length, 0);
    const save = `overlord_hidden_${brainDeath}_${Date.now()}`;
    assert.equal((await game.save(save)).success, true);
    assert.equal((await game.load(save)).success, true);
    let retained = await find(730);
    assert.ok(retained, "save remaps a retained entity instead of respawning it");
    await game.step({ frames: 1560 }); // 28 s since death, including pre-save 2 s.
    assert.equal((await game.entities.detail(retained.id)).has_refs, false, "load must preserve remaining deadline");
    await game.step({ frames: 180 });
    assert.equal((await find(730))?.id, retained.id);
    assert.equal(await hp(retained.id), 120);
    assert.equal((await game.entities.detail(retained.id)).has_refs, true);
    assert.equal((await game.physics.bodies({ entityId: retained.id })).bodies.length, 1, "one authored body must return");
    if (brainDeath === "hidden") {
      await kill(730);
      await retreat();
      await game.step({ frames: 90 });
      assert.equal((await game.entities.detail(retained.id)).has_refs, false);
    }
    await kill(728);
    await game.step({ frames: 5 });
    retained = await find(730);
    assert.ok(retained, "BrainDead never instantly kills its watcher");
    assert.equal(await hp(retained.id), 120);
    assert.equal((await game.entities.detail(retained.id)).has_refs, true);
    await retreat();
    await game.step({ frames: 60 });
    const mortalSave = `overlord_mortal_${brainDeath}_${Date.now()}`;
    assert.equal((await game.save(mortalSave)).success, true);
    assert.equal((await game.load(mortalSave)).success, true);
    await kill(730);
    assert.equal(await find(730), undefined, "saved SlayResult0 must keep normal death-link removal");
    await retreat();
    await game.step({ frames: 1920 });
    assert.equal(await find(730), undefined, "mortal projection must not return");
    assert.equal((await game.info()).player.life_state, "alive", "observation requires live simulation");
  });
}
