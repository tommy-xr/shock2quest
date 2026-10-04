import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUpgradeControl } from "./helpers/weapon-upgrades.js";
import { property, openSettings, elements } from "./helpers/hrm.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";

import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) {
  test(`${vr ? "VR" : "flat"} chooser: cancel, device tiers 1-4, alternate unlock, and save/load`,
    { skip: !enabled, timeout: 240_000 }, async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
    await game.player.setStats({ skills: { modify: 0, standard_weapons: 6 } });
    if (vr) await game.input.set("right_hand.squeeze", 1);
    const spawned = await game.player.spawnItem(-17, { hand: "right" });
    await game.step({ frames: 3 });
    let gun = spawned.entity_id;
    const base = await property(game, gun, "GunDescription");
    const ammo = ammoOf(await game.entities.detail(gun));
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_gun_setting, 0, "unmodified alternate fire is locked");
    const cash = await carriedNaniteTotal(game);
    const choices = ["ExtendedCapacity", "LowMaintenanceI", "LowMaintenanceII", "AlternateFire"];
    for (let tier = 0; tier < choices.length; tier++) {
      const device = await game.player.spawnItem(-1488);
      await game.entities.sendMessage(device.entity_id, { type: "Frob" });
      await game.step({ frames: 20 });
      if (tier === 0) {
        await clickUpgradeControl(game, "upgrade_Laser");
        assert.ok(!(await elements(game)).some(e => e.label === "upgrade_confirm"), "unfinished effects cannot be purchased");
        await clickUpgradeControl(game, "upgrade_cancel");
        assert.equal(await property(game, gun, "Modification"), "0");
        assert.ok((await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id), "cancel preserves the device");
        await game.entities.sendMessage(device.entity_id, { type: "Frob" });
        await game.step({ frames: 3 });
      }
      await clickUpgradeControl(game, `upgrade_${choices[tier]}`);
      assert.ok((await elements(game)).some(e => e.text?.includes("use 1 device")));
      await clickUpgradeControl(game, "upgrade_confirm");
      assert.equal(await property(game, gun, "Modification"), String(tier + 1));
      assert.ok(!(await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id), "exactly the selected device was consumed");
      assert.equal(await carriedNaniteTotal(game), cash, "device installation never charges nanites");
      assert.equal(ammoOf(await game.entities.detail(gun)), ammo, "installation creates no ammunition");
      assert.equal(await property(game, gun, "GunDescription"), base);
    }
    const spare = await game.player.spawnItem(-1488);
    await game.entities.sendMessage(spare.entity_id, { type: "Frob" });
    await game.step({ frames: 3 });
    await clickUpgradeControl(game, "upgrade_ExtendedCapacity");
    assert.ok(!(await elements(game)).some(e => e.label === "upgrade_confirm"), "a fifth installation is unavailable");
    assert.ok((await game.entities.byTemplate(-1488)).some(e => e.id === spare.entity_id));
    for (let n = 0; n < 3 && (await game.ui.state()).mode === "use"; n++) {
      await game.input.trigger("ToggleUseMode"); await game.step({ frames: 3 });
    }
    await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_gun_setting, 1, "purchased alternate fire unlocks the same gun");
    const save = `weapon_upgrade_choices_${vr ? "vr" : "flat"}_${Date.now()}`;
    await game.save(save); await game.load(save); await game.step({ frames: 2 });
    const restored = (await game.info()).player;
    const restoredGun = vr ? restored.right_hand_entity_id : restored.wielded_entity_id;
    assert.ok(restoredGun, "the restored weapon remains held");
    gun = restoredGun;
    assert.equal(await property(game, gun, "Modification"), "4");
    assert.equal(await property(game, gun, "GunDescription"), base);
    assert.equal((await game.info()).player.wielded_gun_setting, 1);
  });
}

test("paid chooser critical failure breaks the weapon without installing a choice", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis" });
  await game.player.setStats({ skills: { modify: 6 } });
  const gun = await game.player.spawnItem(-17, { hand: "right" });
  for (let i = 0; i < 5; i++) await game.player.spawnItem("20 Nanites");
  await game.devParams.set("hrm_force_critical", 1);
  await openSettings(game);
  await clickUpgradeControl(game, "modify");
  await clickUpgradeControl(game, "upgrade_ExtendedCapacity");
  await clickUpgradeControl(game, "upgrade_confirm");
  const before = await carriedNaniteTotal(game);
  await clickUpgradeControl(game, "start-hack");
  assert.ok(await carriedNaniteTotal(game) < before);
  const node = (await elements(game)).find(e => e.label?.startsWith("node-"));
  assert.ok(node?.label);
  await clickUpgradeControl(game, node.label);
  assert.equal(await property(game, gun.entity_id, "ObjectState"), "Broken");
  assert.equal(await property(game, gun.entity_id, "Modification"), "0");
});

test("VR device targets the selected left pistol beside a right-hand shotgun", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: ["--vr"] });
  await game.input.set("right_hand.squeeze", 1);
  await game.input.set("left_hand.squeeze", 1);
  const pistol = await game.player.spawnItem(-17, { hand: "left" });
  const shotgun = await game.player.spawnItem(-19, { hand: "right" });
  await game.step({ frames: 3 });
  await game.input.trigger("ToggleUseMode"); await game.step({ frames: 20 });
  await clickUpgradeControl(game, "select_left_hand");
  const device = await game.player.spawnItem(-1488);
  await game.entities.sendMessage(device.entity_id, { type: "Frob" });
  await game.step({ frames: 3 });
  await clickUpgradeControl(game, "upgrade_LowMaintenanceI");
  await clickUpgradeControl(game, "upgrade_confirm");
  assert.equal(await property(game, pistol.entity_id, "Modification"), "1");
  assert.equal(await property(game, shotgun.entity_id, "Modification"), "0");
  assert.ok(!(await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id));
});
