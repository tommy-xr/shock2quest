import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUpgradeControl, unlockWeaponAlternateFire } from "./helpers/weapon-upgrades.js";
import { property } from "./helpers/hrm.js";
import { fireOnce } from "./helpers/weapon.js";
import { tagValue } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) for (const template of [-17, -18]) {
  test(`${vr ? "VR" : "flat"} silencer on ${template}: purchase, quieter shots, save/load`,
    { skip: !enabled, timeout: 240_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      if (vr) await game.input.set("right_hand.squeeze", 1);
      let gun = (await game.player.spawnItem(template, { hand: "right" })).entity_id;
      await game.step({ frames: 20 });
      const shoot = async () => {
        const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
        await fireOnce(game);
        await game.step({ frames: 120 });
        const sounds = (await game.audio.recent()).sounds.filter(s => s.sequence > sequence && tagValue(s, "event") === "shoot");
        assert.ok(sounds.length, "shot resolves an actual audio sample");
        return sounds[0];
      };
      const ordinary = await shoot();
      const device = await game.player.spawnItem(-1488);
      await game.entities.sendMessage(device.entity_id, { type: "Frob" });
      await game.step({ frames: 20 });
      await clickUpgradeControl(game, "upgrade_Silencer");
      await clickUpgradeControl(game, "upgrade_confirm");
      assert.ok(JSON.parse(await property(game, gun, "WeaponUpgrades")).choices.includes("Silencer"));
      assert.ok(!(await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id));
      for (let n = 0; n < 3 && (await game.ui.state()).mode === "use"; n++) {
        await game.input.trigger("ToggleUseMode"); await game.step({ frames: 3 });
      }
      const quiet = await shoot();
      assert.ok(Math.abs(quiet.gain / ordinary.gain - 0.25) < 0.0001, `actual sink gains ${ordinary.gain} -> ${quiet.gain}`);
      const save = `silencer_${vr}_${template}_${Date.now()}`;
      await game.save(save); await game.load(save); await game.step({ frames: 3 });
      const player = (await game.info()).player;
      gun = (vr ? player.right_hand_entity_id : player.wielded_entity_id)!;
      assert.ok(JSON.parse(await property(game, gun, "WeaponUpgrades")).choices.includes("Silencer"));
      const restored = await shoot();
      assert.equal(restored.gain, quiet.gain);
      await unlockWeaponAlternateFire(game);
      await game.input.trigger("CycleGunSetting"); await game.step({ frames: 2 });
      assert.equal((await game.info()).player.wielded_gun_setting, 1);
      const sequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 30 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 60 });
      const repeated = (await game.audio.recent()).sounds.filter(s => s.sequence > sequence && tagValue(s, "event") === "shoot");
      assert.ok(repeated.length >= 2, "burst/AUTO emits multiple paid shots");
      assert.ok(repeated.every(s => Math.abs(s.gain / ordinary.gain - 0.25) < 0.0001), "every repeated shot is suppressed");
    });
}
