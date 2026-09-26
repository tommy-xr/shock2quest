import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";
import { tagValue } from "./helpers/audio.js";
import { aimVrHandAt, quatFromTo } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true])
  for (const setting of [0, 1])
    for (const pellets of [false, true]) {
      test(
        `${vr ? "VR" : "flat"} shotgun setting ${setting}: ${pellets ? "six pellets" : "one slug"}`,
        { skip: !enabled, timeout: 180_000 },
        async () => {
          await using game = await GameServer.launch({
            mission: "debug_weapons",
            debugFlags: vr ? ["--vr"] : [],
          });
          await game.step({ frames: 5 });
          const gun = await cycleToWeapon(game, (e) => e.template_id === -19, {
            settleFrames: vr ? 90 : 5,
          });
          if (vr) {
            await aimVrHandAt(game, gun.position, 0.45, 1, 0);
            await game.step({ frames: 8 });
            assert.equal(
              (await game.info()).player.right_hand_entity_id,
              gun.id,
            );
            await game.input.set("right_hand.position", [0, 1, -2]);
            await game.input.set(
              "right_hand.rotation",
              quatFromTo([0, 0, -1], [-1, 0, 0]),
            );
            await game.step({ frames: 3 });
          } else {
            await game.input.set("head.look", [0, 0]);
          }
          if (setting) {
            await game.input.trigger("CycleGunSetting");
            await game.step({ frames: 2 });
          }
          await game.input.trigger("EjectClip");
          await game.step({ frames: 2 });
          if (pellets) {
            await game.input.trigger("CycleAmmo");
            await game.step({ frames: 2 });
          }
          await game.player.spawnItem(pellets ? -42 : -43);
          await game.input.trigger("Reload");
          await game.step({ frames: 180 });
          const beforeAmmo = ammoOf(await game.entities.detail(gun.id));
          const sequence =
            (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
          const priorHits = new Set(
            (await game.entities.byTemplate(-3544)).map((e) => e.id),
          );
          await game.input.set("right_hand.trigger", 1);
          await game.step({ frames: 1 });
          await game.input.set("right_hand.trigger", 0);
          let hits = (await game.entities.byTemplate(-3544)).filter(
            (e) => !priorHits.has(e.id),
          );
          for (let frame = 0; frame < 4 && hits.length === 0; frame++) {
            await game.step({ frames: 1 });
            hits = (await game.entities.byTemplate(-3544)).filter(
              (e) => !priorHits.has(e.id),
            );
          }
          assert.equal(
            hits.length,
            pellets ? 6 : 1,
            "one real wall impact per authored projectile",
          );
          assert.equal(
            beforeAmmo - ammoOf(await game.entities.detail(gun.id)),
            setting ? 3 : 1,
            "cost is per shell, not per pellet",
          );
          const sounds = (await game.audio.recent()).sounds.filter(
            (s) => s.sequence > sequence && tagValue(s, "event") === "shoot",
          );
          assert.equal(sounds.length, 1, "one firing sound per shell");
          if (pellets) {
            const width =
              Math.max(...hits.map((e) => e.position[2])) -
              Math.min(...hits.map((e) => e.position[2]));
            const height =
              Math.max(...hits.map((e) => e.position[1])) -
              Math.min(...hits.map((e) => e.position[1]));
            assert.ok(
              width > 0.05 && height > 0.05,
              "independent pitch and heading spread reaches the wall",
            );
            assert.ok(
              width < 3 && height < 3,
              "pattern remains bounded at the far wall",
            );
          }
        },
      );
    }

for (const setting of [0, 1]) {
  test(
    `shotgun setting ${setting}: each pellet applies authored contact damage`,
    { skip: !enabled, timeout: 180_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons" });
      await game.step({ frames: 5 });
      await cycleToWeapon(game, (e) => e.template_id === -19);
      if (setting) {
        await game.input.trigger("CycleGunSetting");
        await game.step({ frames: 2 });
      }
      await game.input.trigger("EjectClip");
      await game.step({ frames: 2 });
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 2 });
      await game.player.spawnItem(-42);
      await game.input.trigger("Reload");
      await game.step({ frames: 180 });
      await game.input.set("head.look", [0, 0]);
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 2 });
      const [target] = await game.entities.byTemplate(-397);
      assert.ok(target);
      // Close enough that all six rays land on creature hitboxes despite spread.
      // Keep the eye outside the capsule and aim toward the torso.
      await game.player.teleport({
        x: target.position[0] + 1.1,
        y: 0,
        z: target.position[2],
      });
      await game.player.aimAt(target, {
        hitbox: "torso",
        visibility: "required",
      });
      await game.step({ frames: 1 });
      const seq = Math.max(
        0,
        ...(await game.messages.recent()).messages.map((m) => m.sequence),
      );
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 1 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 2 });
      const damage = (await game.messages.recent()).messages.filter(
        (m) =>
          m.sequence > seq &&
          m.to.entity_id === target.id &&
          m.payload === "Damage",
      );
      assert.equal(
        damage.length,
        6,
        "six separate pellet contacts reach the target",
      );
      const hp = Number(
        (await game.entities.detail(target.id)).properties.find(
          (p) => p.name === "HitPoints",
        )?.value,
      );
      // Human Vulnerability responds x4 to High Explosive. Even with the
      // existing limb multiplier, six normal pellets kill this 12HP hybrid;
      // one pre-fix projectile does not. Alternate pellets author intensity2.
      assert.equal(hp, 0, "six authored contacts are lethal to the hybrid");
    },
  );
}
