import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { clickUiElement } from "./helpers/ui.js";
import { pullTrigger } from "./helpers/weapon.js";

for (const depleted of [false, true]) {
  test(`ElectroPsi recharges an equipped implant through its ${depleted ? "inventory item after depletion" : "socket"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });
    await game.devParams.set("cheat", 1);
    await game.step({ frames: 5 });
    const implant = (await game.player.spawnItem(-102)).entity_id; // EndurBoost
    const property = async (name: string) =>
      (await game.entities.detail(implant)).properties.find(p => p.name === name)?.value;
    const inventoryItem = async () => {
      const ui = await game.ui.state();
      const item = ui.strip?.elements.find(e =>
        e.entity_id === implant && e.kind === "button");
      assert.ok(item, JSON.stringify({ implant, mode: ui.mode, strip: ui.strip }));
      return item;
    };
    const baseMaximum = (await game.info()).player.max_hit_points!;
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
    const item = await inventoryItem();
    await clickUiElement(game, item);
    await clickUiElement(game, item);
    assert.equal(await property("ImplantSlot"), "0");
    const poweredMaximum = (await game.info()).player.max_hit_points!;
    assert.ok(poweredMaximum > baseMaximum, "equipping EndurBoost increases maximum HP");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });

    if (depleted) {
      // Exercise the authored drain, including the zero-charge transition.
      while (Number(await property("Energy")) > 0) {
        await game.step({ frames: Math.min(10, Number(await property("Energy"))) * 600 });
      }
      assert.equal((await game.info()).player.max_hit_points, baseMaximum);
      assert.equal(await property("ImplantSlot"), "0", "depletion leaves the socket occupied");
    }

    await selectPsiPower(game, "ElectroPsi");
    const psi = (await game.info()).player.psi_points!;
    await pullTrigger(game);
    await game.step({ frames: 2 });
    assert.match((await game.ui.state()).name_strip ?? "", /Select/);
    const target = depleted ? await inventoryItem() :
      (await game.ui.state()).strip?.elements.find(e =>
        e.entity_id === implant && e.label === "Implant 1");
    assert.ok(target, "equipped implant must remain targetable");
    await clickUiElement(game, target);
    assert.ok(Number(await property("Energy")) > (depleted ? 0 : 100));
    assert.equal(await property("ImplantSlot"), "0", "targeting must not unequip the implant");
    assert.equal((await game.info()).player.psi_points, psi - 3);
    assert.equal((await game.info()).player.max_hit_points, poweredMaximum,
      "recharge immediately restores the equipped implant's derived health pool");
  });
}
