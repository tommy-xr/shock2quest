import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickElement } from "./helpers/os-upgrade.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";
import { closeSettings, elements, openSettings, property, winBoard } from "./helpers/hrm.js";

// A Broken gun's settings panel offers REPAIR in MODIFY's place. A won paid
// board sets the gun back to Normal with 10 more condition.
const enabled = process.env.SHOCK2_E2E === "1";
const PISTOL = -17;

async function breakGun(game: GameServer, gun: number, condition: number) {
  await game.entities.sendMessage(gun, { type: "SetObjectState", state: "Broken" });
  await game.entities.sendMessage(gun, { type: "SetGunCondition", condition });
  await game.step({ frames: 1 });
  assert.equal(await property(game, gun, "ObjectState"), "Broken");
}
function repairButton(els: Awaited<ReturnType<typeof elements>>) {
  return els.find((e) => e.label === "repair");
}
test(
  "a broken pistol is repaired on the paid board from its settings panel",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.player.setStats({ skills: { repair: 6 }, cyber_affinity: 6 });
    for (let i = 0; i < 5; i++) await game.player.spawnItem("20 Nanites");
    await game.player.spawnItem(PISTOL);
    await game.input.trigger("EquipPistol");
    await game.step({ frames: 3 });
    const gun = (await game.info()).player.wielded_entity_id!;

    await openSettings(game);
    assert.ok(!repairButton(await elements(game)), "a working gun offers no repair");
    await closeSettings(game);

    await breakGun(game, gun, 30);
    await openSettings(game);
    const button = repairButton(await elements(game));
    assert.ok(button, JSON.stringify(await elements(game)));
    assert.ok(
      !(await elements(game)).some((e) => e.label === "modify"),
      "a broken gun offers repair in modify's place",
    );
    await clickElement(game, button);
    assert.ok(
      (await elements(game)).some((e) => e.texture === "iface/repair.pcx"),
      "the board draws the repair art",
    );
    assert.ok(
      (await elements(game)).some((e) => e.texture === "payr.pcx"),
      "an unpaid repair board shows PAYR",
    );

    const balance = await carriedNaniteTotal(game);
    await winBoard(game);
    const won = (await elements(game)).map((e) => e.texture);
    assert.ok(won.includes("winr.pcx"), `a won repair board shows WINR: ${won}`);
    assert.ok(!won.includes("winh.pcx"), "not the hack board's overlay");
    assert.ok(balance > (await carriedNaniteTotal(game)), "each attempt is paid");
    assert.equal(await property(game, gun, "ObjectState"), "Normal");
    assert.equal((await game.info()).player.wielded_gun_condition, 40);

    // Reopening for a newly broken gun starts at the settings, not the old
    // won board.
    await closeSettings(game);
    await breakGun(game, gun, 30);
    await openSettings(game);
    assert.ok(repairButton(await elements(game)), "a fresh open offers repair again");
  },
);
