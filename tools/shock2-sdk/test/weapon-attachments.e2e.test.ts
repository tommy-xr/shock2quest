import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUpgradeControl } from "./helpers/weapon-upgrades.js";
import { property } from "./helpers/hrm.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const vr of [false, true]) for (const attachment of ["Flashlight", "Laser"]) {
  const key = `${attachment.toLowerCase()}_enabled`;
  const toggle = `toggle_${attachment.toLowerCase()}`;
  test(`${vr ? "VR" : "flat"} ${attachment}: purchase, targeted switch, and save/load`,
    { skip: !enabled, timeout: 240_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci2.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.player.setStats({ skills: { standard_weapons: 6 } });
      if (vr) await game.input.set("right_hand.squeeze", 1);
      let gun = (await game.player.spawnItem(-17, { hand: "right" })).entity_id;
      const device = await game.player.spawnItem(-1488);
      await game.entities.sendMessage(device.entity_id, { type: "Frob" });
      await game.step({ frames: 20 });
      await clickUpgradeControl(game, `upgrade_${attachment}`);
      await clickUpgradeControl(game, "upgrade_confirm");
      const state = async () => JSON.parse(await property(game, gun, "WeaponUpgrades"));
      assert.equal((await state())[key], true, "installation switches the light on");
      assert.ok(!(await game.entities.byTemplate(-1488)).some(e => e.id === device.entity_id));
      await clickUpgradeControl(game, toggle);
      assert.equal((await state())[key], false);
      const save = `weapon_${attachment}_${vr ? "vr" : "flat"}_${Date.now()}`;
      await game.save(save);
      await game.load(save);
      await game.step({ frames: 3 });
      const player = (await game.info()).player;
      const restored = vr ? player.right_hand_entity_id : player.wielded_entity_id;
      assert.ok(restored);
      gun = restored;
      assert.equal((await state())[key], false, "off preference survives save/load");
      if ((await game.ui.state()).mode !== "use") {
        await game.input.trigger("ToggleUseMode");
        await game.step({ frames: 20 });
      }
      await clickUpgradeControl(game, "gun_setting");
      await clickUpgradeControl(game, toggle);
      assert.equal((await state())[key], true);
    });
}
