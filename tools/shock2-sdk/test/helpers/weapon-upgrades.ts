import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { findRepoRoot, type GameServer } from "../../src/index.js";

/** Seed persistent choices until the installation UI lands. This deliberately
 * exercises production save/load and stat consumers, not the future UI. */
export async function saveUpgradedWeapon(game: GameServer, name: string): Promise<string> {
  assert.equal((await game.save(name)).success, true);
  const root = findRepoRoot(process.cwd())!;
  const path = [process.env.DARK_ASSET_PATH, join(root, "Data"), join(root, "..", "Data")]
    .filter((p): p is string => Boolean(p))
    .map(p => join(p, "saves", `${name}.sav`)).find(existsSync);
  assert.ok(path, "the runtime must write the fixture save");
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
