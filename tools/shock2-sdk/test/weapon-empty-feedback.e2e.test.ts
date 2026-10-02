import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, cycleToWeapon, pullTrigger, waitForShotReady } from "./helpers/weapon.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { selectPsiPower } from "./helpers/psi.js";

const enabled = process.env.SHOCK2_E2E === "1";
const guns = [
  [-17, "Pistol", "out_pist"], [-18, "Assault Rifle", "out_pist"],
  [-19, "Shotgun", "out_sg"], [-21, "Grenade Launcher", "out_gren"],
  [-22, "Laser Pistol", "out_pist"], [-23, "EMP Rifle", "out_sg"],
  [-25, "Stasis Generator", "out_sg"], [-26, "Fusion Cannon", "out_sg"],
  [-27, "Worm Launcher", "out_gren"], [-29, "Viral Proliferator", "out_gren"],
] as const;

for (const vr of [false, true]) {
  test(`authored empty feedback across all guns (${vr ? "VR" : "flat"})`, {
    skip: !enabled, timeout: 600_000,
  }, async t => {
    for (const [template, name, sample] of guns) await t.test(name, async () => {
      // Separate benches keep dropped guns from intercepting the next grab.
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 10 });
      let gun: { id: number };
      if (vr) {
        await game.input.set("left_hand.squeeze", 1);
        const spawned = await game.player.spawnItem(template, { hand: "left" });
        gun = { id: spawned.entity_id };
        await game.step({ frames: 5 });
        assert.equal((await game.info()).player.wielded_entity_id, gun.id);
      } else {
        gun = await cycleToWeapon(game, e => e.template_id === template);
      }
      let sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      await game.input.trigger("EjectClip");
      await game.step({ frames: 5 });
      assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === "bb08").length, 0,
        "ejection never announces depletion");
      // Energy weapons retain their charge on EjectClip. Drain it through
      // accepted shots; the laser's last two units cannot pay its cost of 3.
      if ([-22, -23].includes(template)) {
        const cost = template === -22 ? 3 : 2;
        while (ammoOf(await game.entities.detail(gun.id)) >= cost) {
          const before = ammoOf(await game.entities.detail(gun.id));
          await waitForShotReady(game);
          await pullTrigger(game, vr ? "left" : "right");
          assert.equal(ammoOf(await game.entities.detail(gun.id)), before - cost);
        }
        await waitForShotReady(game);
        assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === "bb08").length,
          template === -22 ? 0 : 1, "depletion requires spending exactly the final charge");
      } else {
        assert.equal(ammoOf(await game.entities.detail(gun.id)), 0, "eject emptied the gun");
      }
      const remaining = ammoOf(await game.entities.detail(gun.id));
      sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      for (let i = 0; i < 3; i++) await pullTrigger(game, vr ? "left" : "right");
      const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > sequence);
      assert.equal(sounds.filter(s => s.sample === sample &&
        s.tags.some(([k, v]) => k === "event" && v === "outofammo")).length, 3);
      assert.equal(sounds.filter(s => s.sample === "bb08").length, 0, "ejection and dry fire are not depletion transitions");
      assert.equal(ammoOf(await game.entities.detail(gun.id)), remaining);
      // A held trigger is not a stream of new attempts.
      await game.input.set(`${vr ? "left" : "right"}_hand.trigger`, 1);
      await game.step({ frames: 90 });
      await game.input.set(`${vr ? "left" : "right"}_hand.trigger`, 0);
      await game.step({ frames: 2 });
      assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === sample).length, 4);
    });
  });

  test(`insufficient psi speaks once per pull without starting a charge (${vr ? "VR" : "flat"})`, {
    skip: !enabled, timeout: 300_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 10 });
    if (vr) {
      const [amp] = await game.entities.byTemplate(-247);
      await aimVrHandAt(game, amp.position, .35, 1);
      await game.step({ frames: 8 });
      assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
    }
    const points = (await game.info()).player.psi_points!;
    for (let i = 0; i < points; i++) await pullTrigger(game);
    assert.equal((await game.info()).player.psi_points, 0);
    for (const power of ["Cryokinesis", "Codebreaker"]) {
      await selectPsiPower(game, power);
      const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      for (let i = 0; i < 2; i++) {
        await game.input.set("right_hand.trigger", 1);
        await game.step({ frames: 150 });
        await game.input.set("right_hand.trigger", 0);
        await game.step({ frames: 2 });
      }
      assert.equal((await game.audio.recent()).sounds.filter(s => s.sequence > sequence && s.sample === "bb10").length, 2, power);
      assert.equal((await game.info()).player.psi_points, 0);
    }
  });
}
