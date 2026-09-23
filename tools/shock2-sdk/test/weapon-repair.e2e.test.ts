import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickElement } from "./helpers/os-upgrade.js";
import { carriedNaniteTotal } from "./helpers/nanites.js";

// A Broken gun's settings panel offers REPAIR in MODIFY's place. A won paid
// board sets the gun back to Normal with 10 more condition.
const enabled = process.env.SHOCK2_E2E === "1";
const PISTOL = -17;

async function elements(game: GameServer) {
  return (await game.ui.state()).active_panel!.elements;
}
async function openSettings(game: GameServer) {
  if ((await game.ui.state()).mode !== "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
  }
  const button = (await game.ui.state()).readout.find((e) => e.label === "gun_setting");
  assert.ok(button, "the ammo readout offers SETTING");
  await clickElement(game, button);
}
async function closeSettings(game: GameServer) {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });
}
async function property(game: GameServer, gun: number, name: string) {
  return (await game.entities.detail(gun)).properties.find((p) => p.name === name)?.value;
}
async function breakGun(game: GameServer, gun: number, condition: number) {
  await game.entities.sendMessage(gun, { type: "SetObjectState", state: "Broken" });
  await game.entities.sendMessage(gun, { type: "SetGunCondition", condition });
  await game.step({ frames: 1 });
  assert.equal(await property(game, gun, "ObjectState"), "Broken");
}
function repairButton(els: Awaited<ReturnType<typeof elements>>) {
  return els.find((e) => e.label?.startsWith("REPAIR ("));
}
const won = (els: Awaited<ReturnType<typeof elements>>) =>
  els.some((e) => /win[hm]\.pcx/.test(e.texture ?? ""));

/** Play paid boards through the real UI until one is won. */
async function winBoard(game: GameServer) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const start = (await elements(game)).find(
      (e) => e.label === "start-hack" || e.label === "reset-hack",
    );
    assert.ok(start, "a paid attempt remains available");
    await clickElement(game, start);
    for (const node of (await elements(game)).filter((e) => e.label?.startsWith("node-"))) {
      const current = await elements(game);
      if (won(current) || current.some((e) => /fail[hm]\.pcx/.test(e.texture ?? ""))) break;
      const overlay = current.find(
        (e) => e.kind === "image" && e.rect[0] === node.rect[0] && e.rect[1] === node.rect[1],
      );
      if (!overlay) await clickElement(game, node);
    }
    if (won(await elements(game))) return;
  }
  assert.fail("20 paid repair attempts did not win");
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
      !(await elements(game)).some((e) => e.label?.startsWith("MODIFY")),
      "a broken gun offers repair in modify's place",
    );
    await clickElement(game, button);
    assert.ok(
      (await elements(game)).some((e) => e.texture === "iface/repair.pcx"),
      "the board draws the repair art",
    );

    const balance = await carriedNaniteTotal(game);
    await winBoard(game);
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
