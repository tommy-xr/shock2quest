import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { savedGamePath } from "./save.js";
import { type GameServer } from "../../src/index.js";

/** Seed persistent choices until the installation UI lands. This deliberately
 * exercises production save/load and stat consumers, not the future UI. */
export async function saveUpgradedWeapon(game: GameServer, name: string): Promise<string> {
  assert.equal((await game.save(name)).success, true);
  const path = savedGamePath(name);
  const save = JSON.parse(readFileSync(path, "utf8"));
  const held = save.global_data.held_items;
  const gun = held.entity_in_left_hand ?? held.entity_in_right_hand;
  assert.ok(Number.isSafeInteger(gun), "the fixture must hold a weapon with an exact saved ID");
  assert.ok(held.held_entities.properties["P$GunState"][gun]);
  held.entity_in_left_hand = null;
  held.entity_in_right_hand = gun;
  held.held_entities.weapon_upgrades[String(gun)] = {
    choices: ["ExtendedCapacity", "AlternateFire", "LowMaintenanceI", "LowMaintenanceII"],
    flashlight_enabled: false,
    laser_enabled: false,
  };
  writeFileSync(path, JSON.stringify(save));
  return path;
}

/** Click either presentation's actual chooser, preserving a held VR weapon. */
export async function clickUpgradeControl(game: GameServer, label: string): Promise<void> {
  const ui = await game.ui.state();
  const element = [...(ui.active_panel?.elements ?? []), ...ui.readout].find(e => e.label === label);
  assert.ok(element, `upgrade control ${label}`);
  if (ui.panel_pose) {
    const { aimVrHandAtCanvas } = await import("./vr-hand.js");
    const { canvasCenter } = await import("./ui.js");
    for (const trigger of [0, 1, 0]) {
      await aimVrHandAtCanvas(game, ui.panel_pose, canvasCenter(element), { trigger, squeeze: 1 });
      await game.step({ frames: 3 });
    }
  } else {
    const { clickElement } = await import("./os-upgrade.js");
    await clickElement(game, element);
  }
}

/** Existing mode/animation fixtures purchase the new prerequisite through the
 * real device chooser, without replacing the weapon or its ammunition. */
export async function unlockWeaponAlternateFire(game: GameServer): Promise<void> {
  const player = (await game.info()).player;
  const gun = player.wielded_entity_id ?? player.right_hand_entity_id;
  assert.ok(gun != null, "alternate-fire fixture holds a gun");
  const properties = (await game.entities.detail(gun)).properties;
  const saved = properties.find(p => p.name === "WeaponUpgrades");
  if (saved && JSON.parse(saved.value).choices.includes("AlternateFire")) return;
  // Mode/projectile fixtures model completed research; research gating itself
  // is tested separately. Biological weapons otherwise begin Unresearched.
  if (properties.find(p => p.name === "ObjectState")?.value === "Unresearched") {
    await game.entities.sendMessage(gun, { type: "SetObjectState", state: "Normal" });
    await game.step({ frames: 2 });
  }
  const input = await game.input.state();
  const device = await game.player.spawnItem(-1488);
  await game.entities.sendMessage(device.entity_id, { type: "Frob" });
  await game.step({ frames: 20 });
  await clickUpgradeControl(game, "upgrade_AlternateFire");
  await clickUpgradeControl(game, "upgrade_confirm");
  for (let n = 0; n < 3 && (await game.ui.state()).mode === "use"; n++) {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 3 });
  }
  for (const hand of ["left", "right"] as const) {
    const pose = input[`${hand}_hand`];
    await game.input.set(`${hand}_hand.position`, pose.position);
    // Input snapshots and hand input patches both use xyzw (camera placement
    // has a different convention). Preserve the fixture's exact aiming pose.
    await game.input.set(`${hand}_hand.rotation`, pose.rotation);
    await game.input.set(`${hand}_hand.squeeze`, pose.squeeze_value);
    await game.input.set(`${hand}_hand.trigger`, pose.trigger_value);
  }
  await game.step({ frames: 20 });
}
