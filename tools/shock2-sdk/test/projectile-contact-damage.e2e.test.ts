import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// Shipped sources: Standard Bullet -> Standard Impact @4; Laser Shot ->
// Energy Stim @2; Stasis Shot -> Stasis @8. The hybrid responds x1 to the
// first two. This pistol aim crosses a limb (x0.75); the physical laser hits
// the capsule. Stasis authors Freeze/add_metaprop, not damage.
for (const { name, weapon, projectile, expectedHp } of [
  { name: "pistol", weapon: -17, projectile: -362, expectedHp: 9 },
  { name: "laser", weapon: -22, projectile: -2474, expectedHp: 10 },
  { name: "stasis", weapon: -25, projectile: -1352, expectedHp: 12 },
]) {
  test(
    `${name} contact uses the hybrid's authored damage response`,
    {
      skip: process.env.SHOCK2_E2E !== "1",
      timeout: 180_000,
    },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons" });
      await game.step({ frames: 10 });
      await cycleToWeapon(game, (entity) => entity.template_id === weapon, {
        settleFrames: 10,
      });
      if (name === "stasis") {
        await game.input.trigger("EjectClip");
        await game.step({ frames: 2 });
        await game.player.spawnItem(-41);
        await game.input.trigger("Reload");
        await game.step({ frames: 180 });
        assert.equal((await game.info()).player.reloading, false);
      }
      await game.input.set("head.look", [0, 0]);
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 2 });
      const [target] = await game.entities.byTemplate(-397);
      assert.ok(target);
      await game.player.aimAt(target, {
        hitbox: "torso",
        visibility: "required",
      });
      await game.step({ frames: 1 });
      const before = await game.entities.detail(target.id);
      assert.equal(
        Number(before.properties.find((p) => p.name === "HitPoints")?.value),
        12,
      );
      const sequence = Math.max(
        0,
        ...(await game.messages.recent()).messages.map((m) => m.sequence),
      );
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0);
      let nearTarget = false;
      let impactFrame: number | undefined;
      for (let i = 0; i < 24; i++) {
        const shots = await game.entities.byTemplate(projectile);
        if (nearTarget && shots.length === 0) impactFrame ??= i * 2;
        for (const shot of shots) {
          const distance = Math.hypot(
            ...shot.position.map(
              (value, axis) => value - before.position[axis]!,
            ),
          );
          nearTarget ||= distance < 1.5;
        }
        await game.step({ frames: 2 });
      }
      const after = await game.entities.detail(target.id);
      const hp = Number(
        after.properties.find((p) => p.name === "HitPoints")?.value,
      );
      assert.equal(
        hp,
        expectedHp,
        `${name} must resolve its source through the target receptron`,
      );
      const messages = (await game.messages.recent()).messages.filter(
        (m) => m.sequence > sequence,
      );
      const damage = messages.filter(
        (m) => m.to.entity_id === target.id && m.payload === "Damage",
      );
      if (name === "stasis") {
        assert.ok(nearTarget, "a real stasis projectile must reach the target");
        assert.equal(
          (await game.entities.byTemplate(projectile)).length,
          0,
          "the shot impacts well before reaching the backstop",
        );
        // Collision messages are intentionally omitted from the trace. The
        // live shot reaches the hybrid and disappears within 0.2s, far short
        // of the backstop at x=-12 or its authored lifetime.
        assert.ok(
          impactFrame !== undefined && impactFrame < 12,
          "the observed projectile must impact at the nearby target",
        );
        assert.equal(
          damage.length,
          0,
          "a non-damage contact must not send even a zero-damage AI alert",
        );
      } else {
        assert.equal(
          damage.length,
          1,
          "one shot produces exactly one damage message to the target",
        );
        if (name === "pistol")
          assert.ok(
            damage[0]!.impact?.bone != null,
            "the hit must retain its limb identity",
          );
      }
    },
  );
}
