import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { ammoOf, pullTrigger, waitForShotReady } from "./helpers/weapon.js";
import { property } from "./helpers/hrm.js";
import { saveUpgradedWeapon } from "./helpers/weapon-upgrades.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`saved upgrades drive capacity and per-shot wear in ${vr ? "VR" : "flat"}`, {
    skip: !enabled, timeout: 300_000,
  }, async (t) => {
    const name = `weapon_upgrades_e2e_${vr}_${Date.now()}`;
    let initialAmmo: number;
    let rawDescription: string;
    // Build the same held-weapon fixture in flat mode, then load it in either
    // presentation. Real installation is covered by the later chooser layer.
    {
      await using setup = await GameServer.launch({ mission: "medsci2.mis" });
      await setup.player.setStats({ skills: { standard_weapons: 6 } });
      await setup.player.spawnItem(-17);
      await setup.input.trigger("EquipPistol");
      await setup.step({ frames: 5 });
      const gun = (await setup.info()).player.wielded_entity_id!;
      initialAmmo = ammoOf(await setup.entities.detail(gun));
      rawDescription = await property(setup, gun, "GunDescription");
      const path = await saveUpgradedWeapon(setup, name);
      t.after(() => rmSync(path, { force: true }));
    }
    await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
    assert.equal((await game.load(name)).success, true);
    await game.step({ frames: 2 });
    let player = (await game.info()).player;
    let gun = player.wielded_entity_id!;
    if (vr) gun = player.right_hand_entity_id!;
    assert.ok(gun != null);
    const effective = JSON.parse(await property(game, gun, "EffectiveGunSetting"));
    const base = JSON.parse(rawDescription).settings[0];
    assert.equal(effective.clip, base.clip * 2);
    assert.ok(Math.abs(effective.stim_modifier - (base.stim_modifier || 1) * 1.32) < 0.00001);
    assert.equal(await property(game, gun, "Modification"), "4");
    assert.equal(await property(game, gun, "GunDescription"), rawDescription);
    assert.equal(ammoOf(await game.entities.detail(gun)), initialAmmo, "extra capacity creates no ammunition");
    const condition = Number(await property(game, gun, "Condition"));
    await waitForShotReady(game);
    await pullTrigger(game);
    assert.equal(ammoOf(await game.entities.detail(gun)), initialAmmo - 1);
    assert.ok(Math.abs(Number(await property(game, gun, "Condition")) - (condition - 0.4)) < 0.001);

    if (!vr) {
      // The ordinary reload must use the new capacity, not the raw descriptor.
      await game.player.spawnItem(-31);
      await game.player.spawnItem(-31);
      await game.input.trigger("Reload");
      await game.step({ frames: 180 });
      assert.equal(ammoOf(await game.entities.detail(gun)), effective.clip);
      assert.equal(await property(game, gun, "GunDescription"), rawDescription);
    }
    await game.save(name);
    await game.load(name);
    await game.step({ frames: 2 });
    player = (await game.info()).player;
    gun = vr ? player.right_hand_entity_id! : player.wielded_entity_id!;
    assert.equal(JSON.parse(await property(game, gun, "EffectiveGunSetting")).clip, effective.clip,
      "reload and save/load must not compound capacity");
  });
}
