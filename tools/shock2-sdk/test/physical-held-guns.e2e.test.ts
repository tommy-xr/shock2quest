import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, HttpError } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

for (const hand of ["left", "right"] as const) {
  test(
    `physical ${hand} gun follows the controller, stops at a wall, and drops`,
    {
      skip: process.env.SHOCK2_E2E !== "1",
      timeout: 180_000,
    },
    async () => {
      await using game = await GameServer.launch({
        mission: "debug_weapons",
        debugFlags: ["--vr", "--experimental", "physical_held_items"],
      });
      await game.step({ frames: 10 });
      const gun = await cycleToWeapon(game, (e) => e.template_id === -18, {
        settleFrames: 90,
      });
      await aimVrHandAt(game, gun.position!, 0.45, 1, 0, { hand });
      await game.step({ frames: 8 });
      const heldId = async () => {
        const player = (await game.info()).player;
        return hand === "right"
          ? player.right_hand_entity_id
          : player.wielded_entity_id;
      };
      assert.equal(await heldId(), gun.id);
      await game.input.set("head.rotation", [0, 0, 0, 1]);
      await game.input.set(`${hand}_hand.position`, [0, 1, 0]);
      await game.input.set(`${hand}_hand.rotation`, [
        0,
        Math.SQRT1_2,
        0,
        Math.SQRT1_2,
      ]);
      await game.step({ frames: 90 });
      const body = async () => {
        const bodies = (await game.physics.bodies({ entityId: gun.id })).bodies;
        assert.equal(
          bodies.length,
          1,
          "the held gun retains one physical body",
        );
        return bodies[0]!;
      };
      const raised = await body();
      assert.equal(raised.body_type, "kinematic");
      assert.ok(
        raised.position[1] > 2,
        "actual pickup must lift the gun from its floor pose",
      );
      assert.ok(
        Math.abs(raised.position[0]) < 0.6,
        "gun follows the controller in free space",
      );

      await game.player.teleport({ x: -9, y: 1.244, z: 0 });
      await game.step({ frames: 30 });
      const clear = await body();
      for (let i = 1; i <= 24; i++) {
        await game.input.set(`${hand}_hand.position`, [(-2.8 * i) / 24, 1, 0]);
        await game.step({ frames: 3 });
      }
      await game.step({ frames: 30 });
      const blocked = await body();
      assert.ok(
        blocked.position[0] < clear.position[0] - 0.3,
        "gun advances toward the wall",
      );
      assert.ok(
        blocked.position[0] > -11.5,
        "body origin remains in front of the wall despite controller penetration",
      );
      const draws = (await game.scene.objects({ entityId: gun.id })).objects;
      assert.ok(draws.length > 0);
      for (const draw of draws) {
        assert.ok(
          Math.hypot(...draw.position.map((v, i) => v - blocked.position[i]!)) <
            0.001,
          "rendered gun follows its stopped physics body",
        );
      }
      const ammoBefore = ammoOf(await game.entities.detail(gun.id));
      const healthBefore = (await game.info()).player.hit_points;
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      await game.step({ frames: 15 });
      assert.ok(
        ammoOf(await game.entities.detail(gun.id)) < ammoBefore,
        "the blocked physical gun still fires through the actual holding hand",
      );
      assert.equal(
        (await game.info()).player.hit_points,
        healthBefore,
        "firing the held gun must not damage its owner",
      );
      await game.input.set(`${hand}_hand.position`, [0, 1, 0]);
      await game.step({ frames: 90 });
      const withdrawn = await body();
      assert.ok(
        Math.hypot(
          ...withdrawn.position.map((v, i) => v - clear.position[i]!),
        ) < 0.1,
        "withdrawing the controller frees the gun from the wall",
      );
      await game.input.set(`${hand}_hand.squeeze`, 0);
      await game.step({ frames: 30 });
      assert.notEqual(await heldId(), gun.id);
      assert.equal(
        (await body()).body_type,
        "dynamic",
        "drop restores ordinary world physics",
      );
    },
  );
}

test(
  "a full backpack returns a refused physical gun deposit to dynamic world physics",
  {
    skip: process.env.SHOCK2_E2E !== "1",
    timeout: 180_000,
  },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr", "--experimental", "physical_held_items"],
    });
    await game.step({ frames: 10 });
    let refused = false;
    for (let i = 0; i < 40; i++) {
      try {
        await game.player.spawnItem(-928);
      } catch (error) {
        assert.ok(error instanceof HttpError && error.status === 400);
        refused = true;
        break;
      }
    }
    assert.ok(refused, "nonstacking wrenches must fill the backpack");
    const gun = await cycleToWeapon(game, (e) => e.template_id === -18, {
      settleFrames: 90,
    });
    await aimVrHandAt(game, gun.position!, 0.45, 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
    assert.equal(
      (await game.physics.bodies({ entityId: gun.id })).bodies[0]?.body_type,
      "kinematic",
    );
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const pose = (await game.ui.state()).panel_pose;
    assert.ok(pose);
    await aimVrHandAtCanvas(game, pose, [320, 60], { squeeze: 1 });
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.ok(
      !(await game.player.inventory()).items.some(
        (item) => item.entity_id === gun.id,
      ),
    );
    const bodies = (await game.physics.bodies({ entityId: gun.id })).bodies;
    assert.equal(bodies.length, 1);
    assert.equal(
      bodies[0]!.body_type,
      "dynamic",
      "refusal must remove the held drive from the existing body",
    );
  },
);
