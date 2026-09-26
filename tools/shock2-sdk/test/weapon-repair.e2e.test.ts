import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { UiElement, Vec3 } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";
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

/** Deal the repair board with every node a mine and play one: the forced
 * critical failure destroys the gun. The board must stay up over the wreck,
 * showing LOSER.PCX, offering no new deal and taking no input. */
async function loseRepairBoard(
  game: GameServer,
  gun: number,
  click: (el: UiElement) => Promise<void>,
) {
  await game.devParams.set("hrm_force_critical", 1);
  const repair = repairButton(await elements(game));
  assert.ok(repair, JSON.stringify(await elements(game)));
  await click(repair);
  const start = (await elements(game)).find((e) => e.label === "start-hack");
  assert.ok(start, "the repair board offers START");
  await click(start);
  const node = (await elements(game)).find((e) => e.label?.startsWith("node-"));
  assert.ok(node, "a dealt board offers nodes");
  await click(node);

  assert.ok(
    !(await game.entities.list()).entities.some((e) => e.id === gun),
    "a repair critical failure destroys the gun",
  );
  assert.equal((await game.info()).player.wielded_entity_id ?? null, null);
  const lost = async () => {
    const ui = await game.ui.state();
    assert.equal(ui.active_panel?.name, "Weapon Settings", "the panel stays up");
    const els = ui.active_panel!.elements;
    const textures = els.map((e) => e.texture);
    assert.ok(textures.includes("iface/repair.pcx"), `repair backdrop: ${textures}`);
    assert.ok(textures.includes("loser.pcx"), `LOSER overlay: ${textures}`);
    assert.ok(
      !els.some((e) => e.label === "start-hack" || e.label === "reset-hack"),
      "a lost board offers no new deal",
    );
    return els;
  };
  const els = await lost();
  // Further clicks on the wreck's board are inert.
  await click(els.find((e) => e.label?.startsWith("node-"))!);
  await game.step({ frames: 30 });
  await lost();
  await game.devParams.set("hrm_force_critical", 0);
}

test(
  "a repair critical failure keeps its loss overlay up after the gun is gone (flat)",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.player.setStats({ skills: { repair: 6 }, cyber_affinity: 6 });
    for (let i = 0; i < 5; i++) await game.player.spawnItem("20 Nanites");
    await game.player.spawnItem(PISTOL);
    await game.input.trigger("EquipPistol");
    await game.step({ frames: 3 });
    const gun = (await game.info()).player.wielded_entity_id!;
    await breakGun(game, gun, 30);
    await openSettings(game);
    await loseRepairBoard(game, gun, (el) => clickElement(game, el));

    // Closing works as usual, and the next broken gun gets a fresh panel.
    await closeSettings(game);
    assert.equal((await game.ui.state()).active_panel, null, "the panel closes");
    await game.player.spawnItem(PISTOL);
    await game.input.trigger("EquipPistol");
    await game.step({ frames: 3 });
    const next = (await game.info()).player.wielded_entity_id!;
    await breakGun(game, next, 30);
    await openSettings(game);
    assert.ok(repairButton(await elements(game)), "a fresh panel offers repair");
  },
);

test(
  "a repair critical failure keeps its loss overlay up after the gun is gone (VR)",
  { skip: !enabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    for (let i = 0; i < 5; i++) await game.player.spawnItem("20 Nanites");
    const weapon = await cycleToWeapon(game, (e) => e.template_id === PISTOL);
    await aimVrHandAt(game, weapon.position as Vec3, 0.3);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, weapon.id);
    await breakGun(game, weapon.id, 30);

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const pose = requirePanelPose(await game.ui.state());
    // Park the gun hand off the panel so the free left hand is the pointer.
    await aimVrHandAtCanvas(game, pose, [320, 240], {
      hand: "right",
      squeeze: 1,
      facing: "away",
    });
    await game.step({ frames: 3 });
    const click = (el: UiElement) =>
      clickCanvasWithRay(game, pose, canvasCenter(el), "left");
    const setting = (await game.ui.state()).readout.find((e) => e.label === "gun_setting");
    assert.ok(setting, "the VR readout offers SETTING");
    await click(setting);
    await loseRepairBoard(game, weapon.id, click);
  },
);
