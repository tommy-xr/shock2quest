import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUpgradeControl } from "./helpers/weapon-upgrades.js";
import { property } from "./helpers/hrm.js";
import { ammoOf } from "./helpers/weapon.js";

const enabled = process.env.SHOCK2_E2E === "1";
const ranged = [-17, -18, -19, -21, -22, -23, -25, -26, -27, -29];
for (const vr of [false, true]) for (const template of ranged) {
  test(`${vr ? "VR" : "flat"} ranged upgrade ${template}: capacity, alternate mode, saved state`,
    { skip: !enabled, timeout: 240_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.player.setStats({ skills: { standard_weapons: 6, energy_weapons: 6, heavy_weapons: 6, exotic_weapons: 6 } });
      if (vr) await game.input.set("right_hand.squeeze", 1);
      let gun = (await game.player.spawnItem(template, { hand: "right" })).entity_id;
      // This fixture represents completed research. The normal installation
      // gate still refuses unresearched/broken weapons.
      await game.entities.sendMessage(gun, { type: "SetObjectState", state: "Normal" });
      await game.step({ frames: 5 });
      const raw = await property(game, gun, "GunDescription");
      const base = JSON.parse(raw).settings;
      const innateAlternate = template === -27 || template === -29;
      const choices = ["ExtendedCapacity", innateAlternate ? "LowMaintenanceI" : "AlternateFire"];
      const ammo = ammoOf(await game.entities.detail(gun));
      await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 });
      assert.equal((await game.info()).player.wielded_gun_setting, innateAlternate ? 1 : 0,
        "biological alternate modes are innate; manufactured modes start locked");
      if (innateAlternate) {
        await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 });
      }
      for (const choice of choices) {
        const device = await game.player.spawnItem(-1488);
        await game.entities.sendMessage(device.entity_id, { type: "Frob" }); await game.step({ frames: 20 });
        const elements = (await game.ui.state()).active_panel!.elements;
        assert.equal(elements.some(e => e.label === "upgrade_Silencer"), template === -17 || template === -18);
        if (innateAlternate) assert.ok(!elements.some(e => e.label === "upgrade_AlternateFire"),
          "an innate mode cannot consume an upgrade slot");
        await clickUpgradeControl(game, `upgrade_${choice}`);
        if (template === -25) {
          assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.text?.includes("Stasis duration unchanged")));
        }
        await clickUpgradeControl(game, "upgrade_confirm");
        assert.ok(!(await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id));
      }
      assert.equal(await property(game, gun, "GunDescription"), raw, "retail property bonuses are gone");
      assert.equal(ammoOf(await game.entities.detail(gun)), ammo, "capacity never creates ammo");
      const effective = JSON.parse(await property(game, gun, "EffectiveGunSetting"));
      assert.equal(effective.clip, base[0].clip * 2);
      const damage = template === -25 ? base[0].stim_modifier : (base[0].stim_modifier || 1) * 1.16;
      assert.ok(Math.abs(effective.stim_modifier - damage) < 0.0001);
      for (let n = 0; n < 3 && (await game.ui.state()).mode === "use"; n++) {
        await game.input.trigger("ToggleUseMode"); await game.step({ frames: 3 });
      }
      if (template === -27 || template === -29) {
        for (let n = 0; n < 4; n++) await game.player.spawnItem(template === -27 ? -48 : -1264);
        await game.input.trigger("Reload"); await game.step({ frames: 240 });
        assert.equal(ammoOf(await game.entities.detail(gun)), effective.clip,
          "biological reload fills the expanded magazine from reserve");
      }
      await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 });
      assert.equal((await game.info()).player.wielded_gun_setting, 1);
      assert.equal(JSON.parse(await property(game, gun, "EffectiveGunSetting")).clip, base[1].clip * 2);
      const save = `ranged_${template}_${vr}_${Date.now()}`;
      await game.save(save); await game.load(save); await game.step({ frames: 3 });
      const player = (await game.info()).player;
      gun = (vr ? player.right_hand_entity_id : player.wielded_entity_id)!;
      assert.deepEqual(JSON.parse(await property(game, gun, "WeaponUpgrades")).choices, choices);
      assert.equal(JSON.parse(await property(game, gun, "EffectiveGunSetting")).clip, base[1].clip * 2);
    });
}
for (const vr of [false, true]) for (const template of [-928, -24, -28, -247]) {
  test(`${vr ? "VR" : "flat"} melee/psi ${template} cannot spend a modification device`,
    { skip: !enabled, timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
      if (vr) await game.input.set("right_hand.squeeze", 1);
      const gun = (await game.player.spawnItem(template, { hand: "right" })).entity_id;
      const device = await game.player.spawnItem(-1488);
      await game.entities.sendMessage(device.entity_id, { type: "Frob" }); await game.step({ frames: 20 });
      assert.ok(!(await game.ui.state()).active_panel?.elements.some(e => e.label?.startsWith("upgrade_")));
      assert.ok((await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id));
      assert.ok(!(await game.entities.detail(gun)).properties.some(p => p.name === "WeaponUpgrades"));
    });
}
