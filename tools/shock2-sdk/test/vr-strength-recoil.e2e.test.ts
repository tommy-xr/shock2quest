import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import {
  aimVrHandAt,
  quatConjugate,
  quatMultiply,
  quatRotate,
  sub,
  type Quat,
} from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

for (const [name, template] of [
  ["pistol", -17],
  ["AR", -18],
] as const) {
  test(
    `${name} recoil decreases with Strength and genuine support removes the extra spring`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 },
    async (context) => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: ["--vr", "--experimental", "physical_held_items"],
      });
      await game.step({ frames: 10 });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      const gun = await cycleToWeapon(game, (e) => e.template_id === template, {
        settleFrames: 90,
      });
      await aimVrHandAt(game, gun.position!, 0.45, 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.input.set("right_hand.position", [0, 1, 0]);
      await game.input.set("right_hand.rotation", [
        0,
        Math.SQRT1_2,
        0,
        Math.SQRT1_2,
      ]);
      await game.step({ frames: 180 });
      const head = (await game.info()).player.camera_rotation;
      const body = async () =>
        (await game.physics.bodies({ entityId: gun.id })).bodies[0]!;
      const placeSupportHandAtCurrentSocket = async () => {
        const player = (await game.info()).player;
        const socket = player.hand_grips.find(
          (g) => g.hand === "right",
        )!.support;
        assert.ok(socket, `${name} must expose its authored support socket`);
        const inverse = quatConjugate(player.rotation);
        const p = socket.controller_position;
        const q = socket.controller_rotation;
        await game.input.set(
          "left_hand.position",
          quatRotate(inverse, sub([p.x, p.y, p.z], player.position)),
        );
        await game.input.set(
          "left_hand.rotation",
          quatMultiply(inverse, [q.v.x, q.v.y, q.v.z, q.s] as Quat),
        );
      };
      const support = async (attached: boolean) => {
        await game.input.set("left_hand.squeeze", 0);
        await game.step({ frames: 2 });
        if (attached) {
          await placeSupportHandAtCurrentSocket();
          await game.input.set("left_hand.squeeze", 1);
        }
        await game.step({ frames: 15 });
        assert.equal(
          (await game.info()).player.hand_grips.find((g) => g.hand === "right")!
            .support?.attached ?? false,
          attached,
        );
      };
      const measurements: {
        strength: number;
        supported: boolean;
        back: number;
        curve: number[];
      }[] = [];
      for (const strength of [1, 3, 6]) {
        await game.player.setStats({ strength });
        for (const supported of [false, true]) {
          await support(supported);
          await game.step({ frames: 180 });
          const initial = await body();
          const ammo = ammoOf(await game.entities.detail(gun.id));
          await game.input.set("right_hand.trigger", 1);
          let peakBack = 0;
          const curve: number[] = [];
          for (let frame = 0; frame < 20; frame++) {
            await game.step({ frames: 1 });
            if (frame === 0) await game.input.set("right_hand.trigger", 0);
            const back = (await body()).position[0] - initial.position[0];
            curve.push(back);
            peakBack = Math.max(peakBack, back);
          }
          assert.equal(ammoOf(await game.entities.detail(gun.id)), ammo - 1);
          assert.deepEqual((await game.info()).player.camera_rotation, head);
          measurements.push({ strength, supported, back: peakBack, curve });
          await game.step({ frames: 180 });
          const settled = await body();
          assert.ok(
            Math.hypot(...sub(settled.position, initial.position)) < 0.005,
            "both recoil springs must return to this shot's resting position",
          );
          const relative = quatMultiply(
            quatConjugate(initial.rotation),
            settled.rotation,
          );
          const angle =
            (2 *
              Math.atan2(
                Math.hypot(...relative.slice(0, 3)),
                Math.abs(relative[3]),
              ) *
              180) /
            Math.PI;
          assert.ok(
            angle < 0.1,
            `both recoil springs must restore this shot's resting rotation; ${angle} degrees remain`,
          );
        }
      }
      context.diagnostic(
        JSON.stringify(measurements.map(({ curve, ...result }) => result)),
      );
      const value = (s: number, supported: boolean) =>
        measurements.find((m) => m.strength === s && m.supported === supported)!
          .back;
      const baseline = value(1, true);
      assert.ok(
        baseline > 0.03,
        "actual firing must produce measurable backward recoil",
      );
      for (const m of measurements) {
        const n = m.strength - 1;
        const expected =
          baseline *
          (1 / (1 + 0.1 * n) + (m.supported ? 0 : 1 / (1 + 0.3 * n)));
        assert.ok(
          Math.abs(m.back - expected) < 0.003,
          `${JSON.stringify(m)} expected ${expected}`,
        );
      }
      if (name === "pistol") {
        await support(false);
        await game.step({ frames: 180 });
        await placeSupportHandAtCurrentSocket();
        const initial = await body();
        const reference = measurements.find(
          (m) => m.strength === 6 && !m.supported,
        )!.curve;
        await game.input.set("right_hand.trigger", 1);
        for (let frame = 0; frame < 20; frame++) {
          if (frame === 2) {
            // Acquire the moving socket, not its pre-shot position: backward
            // recoil can already exceed the authored 7 cm grab radius.
            await placeSupportHandAtCurrentSocket();
            await game.input.set("left_hand.squeeze", 1);
          }
          if (frame === 5) await game.input.set("left_hand.squeeze", 0);
          await game.step({ frames: 1 });
          if (frame === 0) await game.input.set("right_hand.trigger", 0);
          if (frame === 3 || frame === 6) {
            assert.equal(
              (await game.info()).player.hand_grips.find(
                (g) => g.hand === "right",
              )!.support!.attached,
              frame === 3,
            );
          }
          const back = (await body()).position[0] - initial.position[0];
          assert.ok(
            Math.abs(back - reference[frame]!) < 0.005,
            `support transition must preserve the existing recoil: frame ${frame}, back ${back}, uninterrupted ${reference[frame]}`,
          );
        }
        assert.deepEqual((await game.info()).player.camera_rotation, head);
      }
    },
  );
}
