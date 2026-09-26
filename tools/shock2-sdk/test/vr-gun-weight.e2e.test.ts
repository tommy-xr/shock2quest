import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type Vec3 } from "../src/index.js";
import {
  add,
  sub,
  aimVrHandAt,
  quatConjugate,
  quatFromTo,
  quatMultiply,
  quatRotate,
  type Quat,
} from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon, muzzleFrameOf } from "./helpers/weapon.js";

for (const [name, template, sag, enabled, hand, overrides] of [
  ["pistol", -17, 1, true, "right", false],
  ["shotgun", -19, 8, true, "right", false],
  ["shotgun left hand", -19, 8, true, "left", false],
  ["shotgun without weight flag", -19, 0, false, "right", false],
  ["AR", -18, 8, true, "right", false],
  ["AR left hand", -18, 8, true, "left", false],
  ["AR without weight flag", -18, 0, false, "right", false],
  ["shotgun overrides", -19, 8, true, "right", true],
] as const) {
  test(
    `${name}: Strength and support remove downward weight around the palm`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 },
    async (context) => {
      await using game = await GameServer.launch({
        mission: overrides ? "debug_weapons" : "medsci1.mis",
        debugFlags: [
          "--vr",
          "--experimental",
          enabled
            ? "physical_held_items,physical_gun_weight"
            : "physical_held_items",
        ],
      });
      await game.step({ frames: 10 });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      const character = (await game.info()).player.stats;
      if (overrides) assert.equal(character?.strength, 6);
      const gun = await cycleToWeapon(game, (e) => e.template_id === template, {
        settleFrames: 90,
      });
      const other = hand === "right" ? "left" : "right";
      await aimVrHandAt(game, gun.position!, 0.45, 1, 0, { hand });
      await game.step({ frames: 8 });
      assert.equal(
        (await game.info()).player[
          hand === "left" ? "wielded_entity_id" : "right_hand_entity_id"
        ],
        gun.id,
      );
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.input.set(`${hand}_hand.position`, [0, 1, 0]);
      await game.input.set(`${hand}_hand.rotation`, [
        0,
        Math.SQRT1_2,
        0,
        Math.SQRT1_2,
      ]);
      await game.step({ frames: 300 });
      const head = (await game.info()).player.camera_rotation;
      const muzzle = async () =>
        muzzleFrameOf(await game.entities.detail(gun.id));
      const pitch = async () =>
        (Math.asin((await muzzle()).forward[1]) * 180) / Math.PI;
      const socket = async () =>
        (await game.info()).player.hand_grips.find((g) => g.hand === hand)!
          .support!;
      const palmIsAnchored = async () => {
        const s = await socket();
        const a = s.primary_palm,
          b = s.visible_primary_palm;
        assert.ok(
          Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z) < 0.002,
          "weight rotates around the primary palm",
        );
        assert.deepEqual((await game.info()).player.camera_rotation, head);
      };
      for (const testLevel of overrides ? [0, 1, 3, 6, 1, 0] : [1, 3, 6]) {
        const strength = testLevel || character!.strength;
        const previous = await pitch();
        if (overrides) {
          await game.devParams.set("gun_strength_override", testLevel);
          assert.deepEqual((await game.info()).player.stats, character);
        } else await game.player.setStats({ strength });
        await game.step({ frames: 1 });
        assert.ok(
          Math.abs((await pitch()) - previous) < 0.5,
          "Strength changes must not snap the gun",
        );
        await game.step({ frames: 300 });
        const expected = (-sag * (6 - strength)) / 5;
        assert.ok(
          Math.abs((await pitch()) - expected) < 0.05,
          `Strength ${strength}: expected ${expected}, got ${await pitch()}`,
        );
        await palmIsAnchored();
        context.diagnostic(
          JSON.stringify({ name, strength, pitch: await pitch() }),
        );
        if (strength !== 1 || !enabled || overrides) continue;

        // Acquire the actual lowered fore-end first, then lift it to level.
        // On long guns the neutral socket is outside the sagged socket's grab
        // radius. Resolve both poses from the live barrel and primary palm.
        const s = await socket();
        const player = (await game.info()).player;
        const level = quatFromTo((await muzzle()).forward, [-1, 0, 0]);
        const anchor: Vec3 = [
          s.primary_palm.x,
          s.primary_palm.y,
          s.primary_palm.z,
        ];
        const p: Vec3 = [
          s.controller_position.x,
          s.controller_position.y,
          s.controller_position.z,
        ];
        const q = s.controller_rotation;
        const inverse = quatConjugate(player.rotation);
        await game.input.set(
          `${other}_hand.position`,
          quatRotate(inverse, sub(p, player.position)),
        );
        await game.input.set(
          `${other}_hand.rotation`,
          quatMultiply(inverse, [q.v.x, q.v.y, q.v.z, q.s] as Quat),
        );
        await game.input.set(`${other}_hand.squeeze`, 1);
        await game.step({ frames: 1 });
        assert.equal(
          (await socket()).attached,
          true,
          "acquire the live lowered support socket",
        );
        await game.input.set(
          `${other}_hand.position`,
          quatRotate(
            inverse,
            sub(
              add(anchor, quatRotate(level, sub(p, anchor))),
              player.position,
            ),
          ),
        );
        await game.input.set(
          `${other}_hand.rotation`,
          quatMultiply(
            inverse,
            quatMultiply(level, [q.v.x, q.v.y, q.v.z, q.s] as Quat),
          ),
        );
        await game.input.set(`${other}_hand.squeeze`, 1);
        let last = await pitch();
        for (let frame = 0; frame < 30; frame++) {
          await game.step({ frames: 1 });
          const current = await pitch();
          assert.ok(
            // Shared support aiming interpolates with a 60 ms time constant:
            // lifting the controller by 8 degrees can move about 2 degrees
            // in one 60 Hz frame, independently of the slower weight spring.
            Math.abs(current - last) < 3,
            `support must interpolate, not apply the whole lift at once: ${last} -> ${current}`,
          );
          last = current;
        }
        assert.equal((await socket()).attached, true);
        await game.step({ frames: 300 });
        assert.ok(
          Math.abs(await pitch()) < 0.1,
          `support removes sag: ${await pitch()}`,
        );
        await palmIsAnchored();
        await game.input.set(`${other}_hand.squeeze`, 0);
        last = await pitch();
        for (let frame = 0; frame < 30; frame++) {
          await game.step({ frames: 1 });
          const current = await pitch();
          assert.ok(
            Math.abs(current - last) < 1,
            "releasing support eases weight back in",
          );
          last = current;
        }
        await game.step({ frames: 300 });
        assert.ok(Math.abs((await pitch()) + sag) < 0.05);
        await palmIsAnchored();

        const ammo = ammoOf(await game.entities.detail(gun.id));
        const before = await pitch();
        await game.input.set(`${hand}_hand.trigger`, 1);
        let peak = before;
        for (let frame = 0; frame < 30; frame++) {
          await game.step({ frames: 1 });
          if (frame === 0) await game.input.set(`${hand}_hand.trigger`, 0);
          peak = Math.max(peak, await pitch());
        }
        assert.equal(ammoOf(await game.entities.detail(gun.id)), ammo - 1);
        assert.ok(
          peak > before + 1,
          "shot recoil remains visible above the downward weight bias",
        );
        await game.step({ frames: 300 });
        assert.ok(
          Math.abs((await pitch()) - before) < 0.05,
          "recoil returns to the weighted rest pose",
        );
        await palmIsAnchored();
      }
    },
  );
}
