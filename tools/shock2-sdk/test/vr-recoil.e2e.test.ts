import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import {
  aimVrHandAt,
  quatConjugate,
  quatMultiply,
  quatNormalize,
  type Quat,
} from "./helpers/vr-hand.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

function rotationDifferenceDegrees(a: Quat, b: Quat): number {
  const relative = quatNormalize(quatMultiply(quatConjugate(a), b));
  return (
    (2 *
      Math.atan2(Math.hypot(...relative.slice(0, 3)), Math.abs(relative[3])) *
      180) /
    Math.PI
  );
}

for (const [name, template, hand] of [
  ["pistol", -17, "right"],
  ["assault rifle", -18, "left"],
] as const) {
  test(
    `VR ${name} recoils after a successful shot and returns to its fixed hand`,
    {
      skip: process.env.SHOCK2_E2E !== "1",
      timeout: 180_000,
    },
    async (context) => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: ["--vr", "--experimental", "physical_held_items"],
      });
      await game.step({ frames: 10 });
      const gun = await cycleToWeapon(game, (e) => e.template_id === template, {
        settleFrames: 90,
      });
      await aimVrHandAt(game, gun.position!, 0.45, 1, 0, { hand });
      await game.step({ frames: 8 });
      const info = await game.info();
      assert.equal(
        hand === "right"
          ? info.player.right_hand_entity_id
          : info.player.wielded_entity_id,
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
      await game.step({ frames: 90 });
      const body = async () =>
        (await game.physics.bodies({ entityId: gun.id })).bodies[0]!;
      const initial = await body();
      const initialHead = (await game.info()).player.camera_rotation;
      const initialAmmo = ammoOf(await game.entities.detail(gun.id));
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      await game.step({ frames: 12 });
      assert.equal(
        ammoOf(await game.entities.detail(gun.id)),
        initialAmmo,
        "insufficient skill refuses the shot",
      );
      const refused = await body();
      assert.deepEqual(
        refused.rotation,
        initial.rotation,
        "refused shots do not kick the gun",
      );
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      let peakAngle = rotationDifferenceDegrees(
        initial.rotation,
        (await body()).rotation,
      );
      await game.step({ frames: 1 });
      peakAngle = Math.max(
        peakAngle,
        rotationDifferenceDegrees(initial.rotation, (await body()).rotation),
      );
      const firedAmmo = ammoOf(await game.entities.detail(gun.id));
      assert.ok(firedAmmo < initialAmmo);
      await game.input.set(`${hand}_hand.trigger`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${hand}_hand.trigger`, 0);
      peakAngle = Math.max(
        peakAngle,
        rotationDifferenceDegrees(initial.rotation, (await body()).rotation),
      );
      assert.equal(
        ammoOf(await game.entities.detail(gun.id)),
        firedAmmo,
        "cooldown refuses a second immediate shot",
      );
      for (let frame = 0; frame < 12; frame++) {
        await game.step({ frames: 1 });
        peakAngle = Math.max(
          peakAngle,
          rotationDifferenceDegrees(initial.rotation, (await body()).rotation),
        );
      }
      const kicked = await body();
      assert.ok(
        kicked.position[0] > initial.position[0] + 0.005,
        "gun kicks backward from the barrel direction",
      );
      assert.ok(
        peakAngle > 1,
        `Agility 1 leaves authored angular recoil; measured peak ${peakAngle} degrees`,
      );
      // AR's minimum randomized pitch is 1.626 degrees. Its fast return can
      // already fall below 0.573 degrees (quaternion Z=.005) at 15 frames;
      // inspect the early peak instead of treating that valid recovery as no kick.
      context.diagnostic(
        `peak=${peakAngle.toFixed(4)}deg, late=${rotationDifferenceDegrees(initial.rotation, kicked.rotation).toFixed(4)}deg, oldLateZ=${Math.abs(kicked.rotation[2]).toFixed(6)}`,
      );
      assert.deepEqual(
        (await game.info()).player.camera_rotation,
        initialHead,
        "gun recoil does not jolt the head",
      );
      await game.step({ frames: 300 });
      const settled = await body();
      assert.ok(
        Math.hypot(
          ...settled.position.map((v, i) => v - initial.position[i]!),
        ) < 0.005,
      );
      assert.ok(
        rotationDifferenceDegrees(initial.rotation, settled.rotation) < 0.1,
        "spring returns to the unchanged controller pose",
      );
    },
  );
}

test(
  "flat weapon firing does not create a held recoil body or move the camera",
  { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--experimental", "physical_held_items"],
    });
    await game.step({ frames: 10 });
    await game.player.setStats({ skills: { standard_weapons: 6 } });
    await game.player.spawnItem("Pistol");
    await game.input.trigger("EquipPistol");
    await game.step({ frames: 5 });
    const initial = (await game.info()).player;
    assert.ok(initial.wielded_entity_id);
    const ammo = ammoOf(await game.entities.detail(initial.wielded_entity_id));
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 15 });
    assert.ok(
      ammoOf(await game.entities.detail(initial.wielded_entity_id)) < ammo,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: initial.wielded_entity_id }))
        .bodies.length,
      0,
    );
    assert.deepEqual(
      (await game.info()).player.camera_rotation,
      initial.camera_rotation,
    );
  },
);
