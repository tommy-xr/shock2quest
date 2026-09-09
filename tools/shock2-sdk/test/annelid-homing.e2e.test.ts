import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

for (const setting of [0, 1]) {
  test(
    `annelid mode ${setting} homes toward a hybrid with both target flags`,
    {
      skip: process.env.SHOCK2_E2E !== "1",
      timeout: 180_000,
    },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons" });
      try {
        await game.step({ frames: 10 });
        await game.player.setStats({ skills: { exotic_weapons: 6 } });
        await cycleToWeapon(game, (e) => e.template_id === -27, {
          settleFrames: 10,
        });
        await game.player.spawnItem(-1264);
        await game.input.trigger("Reload");
        await game.step({ frames: 180 });
        for (let i = 0; (await game.info()).player.reloading && i < 20; i++)
          await game.step({ frames: 60 });
        assert.equal((await game.info()).player.reloading, false);
        if (setting) {
          await game.input.trigger("CycleGunSetting");
          await game.step({ frames: 2 });
        }
        await game.player.teleport({ x: -2, y: 0, z: 1.5 });
        await game.input.set("head.look", [0, 0]);
        await game.input.trigger("SpawnDebugMonster");
        await game.step({ frames: 1 });
        const [target] = await game.entities.byTemplate(-397);
        assert.ok(target);
        await game.player.teleport({ x: 0, y: 0, z: 0 });
        await game.step({ frames: 2 });
        await game.input.set("right_hand.trigger", 1);
        await game.step({ frames: 1 });
        await game.input.set("right_hand.trigger", 0);
        const [shot] = await game.entities.byTemplate(setting ? -3502 : -1356);
        assert.ok(shot);
        const [body] = (await game.physics.bodies({ entityId: shot.id }))
          .bodies;
        assert.ok(body);
        await game.step({ frames: 16 });
        const [turned] = (await game.physics.bodies({ entityId: shot.id }))
          .bodies;
        assert.ok(
          turned,
          "projectile remains in flight beyond first homing pulse",
        );
        const change = Math.hypot(
          ...turned.velocity.map((v, i) => v - body.velocity[i]!),
        );
        assert.ok(
          change > 0.1,
          `both modes must steer toward the off-axis hybrid; velocity delta=${change}`,
        );
        assert.ok(
          Math.abs(
            Math.hypot(...turned.velocity) - Math.hypot(...body.velocity),
          ) < 0.01,
          "homing preserves launch speed",
        );
        assert.ok(
          (await game.scene.objects({ entityId: shot.id })).objects.length > 0,
          "the annelid rocket model is actually submitted to the renderer",
        );
        // Authored lifetime is five seconds even when no compatible target exists.
        await game.step({ frames: 360 });
        assert.equal(
          (await game.entities.byTemplate(setting ? -3502 : -1356)).length,
          0,
        );
      } catch (error) {
        console.error(game.logs().slice(-30).join("\n"));
        throw error;
      }
    },
  );
}

test(
  "a turning annelid missile preserves velocity and lock through a mission save",
  {
    skip: process.env.SHOCK2_E2E !== "1",
    timeout: 180_000,
  },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 30 });
    await game.player.setStats({ skills: { exotic_weapons: 6 } });
    await game.player.spawnItem("Worm Launcher");
    await game.input.trigger("EquipWormLauncher");
    await game.step({ frames: 5 });
    await game.player.spawnItem(-1264);
    await game.input.trigger("Reload");
    await game.step({ frames: 180 });
    for (let i = 0; (await game.info()).player.reloading && i < 20; i++)
      await game.step({ frames: 60 });
    await game.input.set("head.look", [0, 0]);
    const home = (await game.info()).player.position;
    // Spawn far enough beyond the muzzle that the target fits the pitch cone.
    await game.player.teleport({ x: home[0], y: home[1], z: home[2] + 4 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 1 });
    await game.player.teleport({ x: home[0], y: home[1], z: home[2] });
    await game.input.set("head.look", [15, 0]);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    const [shot] = await game.entities.byTemplate(-1356);
    assert.ok(shot);
    const [initial] = (await game.physics.bodies({ entityId: shot.id })).bodies;
    await game.step({ frames: 16 });
    const [turned] = (await game.physics.bodies({ entityId: shot.id })).bodies;
    assert.ok(initial && turned);
    assert.ok(
      Math.hypot(...turned.velocity.map((v, i) => v - initial.velocity[i]!)) >
        0.1,
      "fixture saves after a real homing turn",
    );
    const slot = `homing_${Date.now()}`;
    assert.equal((await game.save(slot)).success, true);
    assert.equal((await game.load(slot)).success, true);
    const [restored] = await game.entities.byTemplate(-1356);
    assert.ok(restored);
    const [body] = (await game.physics.bodies({ entityId: restored.id }))
      .bodies;
    assert.ok(body);
    assert.ok(
      Math.hypot(...body.velocity.map((v, i) => v - turned.velocity[i]!)) <
        0.001,
      "saved world-space direction and speed survive reconstruction before the next physics step",
    );
    await game.step({ frames: 13 });
    const [later] = (await game.physics.bodies({ entityId: restored.id }))
      .bodies;
    assert.ok(later);
    assert.ok(
      Math.hypot(...later.velocity.map((v, i) => v - body.velocity[i]!)) > 0.1,
      "the remapped target remains locked and drives the next steering pulse",
    );
  },
);
