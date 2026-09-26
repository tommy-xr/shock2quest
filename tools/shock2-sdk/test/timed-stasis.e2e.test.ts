import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { cycleToWeapon, fireOnce } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";

async function remaining(
  game: GameServer,
  id: number,
): Promise<number | undefined> {
  const value = (await game.entities.detail(id)).properties.find(
    (p) => p.name === "StasisRemaining",
  )?.value;
  return value === undefined ? undefined : Number(value);
}

for (const scenario of ["refresh", "save", "area", "death"] as const) {
  test(
    `actual stasis shot: ${scenario}`,
    { skip: !enabled, timeout: 240_000 },
    async () => {
      await using game = await GameServer.launch({
        mission: scenario === "save" ? "earth.mis" : "debug_weapons",
      });
      await game.step({ frames: 10 });
      if (scenario === "save") {
        await game.player.setStats({
          skills: { energy_weapons: 6, heavy_weapons: 6, exotic_weapons: 6 },
        });
        await game.player.spawnItem(-25);
        await game.input.trigger("EquipStasisFieldGenerator");
        await game.step({ frames: 10 });
      } else {
        await cycleToWeapon(game, (e) => e.template_id === -25);
      }
      if (scenario === "area") {
        await game.input.trigger("CycleGunSetting");
        await game.step({ frames: 2 });
      }
      await game.input.trigger("EjectClip");
      await game.step({ frames: 2 });
      await game.player.spawnItem(-41);
      await game.input.trigger("Reload");
      await game.step({ frames: 180 });
      await game.input.set("head.look", [0, 0]);
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 2 });
      let [target] = await game.entities.byTemplate(-397);
      assert.ok(target);
      await game.player.aimAt(target, {
        hitbox: "torso",
        visibility: "required",
      });
      await game.step({ frames: 1 });
      await fireOnce(game);
      for (
        let i = 0;
        i < 30 && (await remaining(game, target.id)) === undefined;
        i++
      ) {
        await game.step({ frames: 2 });
      }
      const duration = await remaining(game, target.id);
      assert.ok(
        duration !== undefined && duration > 0 && duration <= 8,
        "a real projectile must freeze the target",
      );
      if (scenario !== "area")
        assert.ok(duration > 7.8, "normal stasis lasts eight seconds");
      assert.equal(
        Number(
          (await game.entities.detail(target.id)).properties.find(
            (p) => p.name === "HitPoints",
          )?.value,
        ),
        12,
      );

      if (scenario === "death") {
        await game.entities.sendMessage(target.id, {
          type: "Damage",
          amount: 100,
        });
        await game.step({ frames: 2 });
        assert.equal(
          await remaining(game, target.id),
          undefined,
          "death must release stasis immediately",
        );
        return;
      }
      await game.step({ frames: 60 });
      const frozen = await game.entities.animation(target.id);
      assert.ok(frozen);
      await game.step({ frames: 120 });
      const held = await game.entities.animation(target.id);
      assert.ok(held);
      // Gravity/contact resolution may translate the body; joint offsets must
      // retain the same pose rather than advancing animation.
      const poseError = (joints: number[][]) =>
        Math.max(
          ...joints.flatMap((joint, i) =>
            joint.map((v, axis) =>
              Math.abs(
                v -
                  joints[0]![axis]! -
                  (frozen.joints[i]![axis]! - frozen.joints[0]![axis]!),
              ),
            ),
          ),
        );
      assert.ok(
        poseError(held.joints) < 0.0001,
        "the frozen pose and hitboxes must not advance",
      );

      if (scenario === "refresh") {
        await game.player.aimAt(target.id, {
          hitbox: "torso",
          visibility: "required",
        });
        await fireOnce(game);
        await game.step({ frames: 12 });
        const refreshed = await remaining(game, target.id);
        assert.ok(
          refreshed !== undefined && refreshed > 7.5 && refreshed <= 8,
          "reapplication replaces the remaining duration",
        );
      }
      if (scenario === "save") {
        const before = await remaining(game, target.id);
        const slot = `timed_stasis_${Date.now()}`;
        assert.equal((await game.save(slot)).success, true);
        assert.equal((await game.load(slot)).success, true);
        [target] = await game.entities.byTemplate(-397);
        assert.ok(target, "discover the restored target by stable template");
        const restored = await remaining(game, target.id);
        assert.ok(
          restored !== undefined &&
            before !== undefined &&
            Math.abs(restored - before) < 0.05,
          "load preserves remaining duration",
        );
        await game.step({ frames: 2 });
        const pose = await game.entities.animation(target.id);
        assert.ok(pose);
        assert.ok(
          poseError(pose.joints) < 0.0001,
          "load preserves the frozen pose",
        );
      }
      const left = await remaining(game, target.id);
      assert.ok(left !== undefined && left > 0);
      await game.step({ frames: Math.max(1, Math.floor(left * 60) - 2) });
      assert.ok(
        (await remaining(game, target.id)) !== undefined,
        "stasis remains until its deadline",
      );
      await game.step({ frames: 4 });
      assert.equal(
        await remaining(game, target.id),
        undefined,
        "stasis expires at the deadline",
      );
      const thawed = await game.entities.animation(target.id);
      await game.step({ frames: 30 });
      assert.notDeepEqual(
        (await game.entities.animation(target.id))?.joints,
        thawed?.joints,
        "animation resumes after expiry",
      );
    },
  );
}
