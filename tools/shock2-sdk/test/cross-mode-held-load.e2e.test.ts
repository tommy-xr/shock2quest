import assert from "node:assert/strict";
import { test } from "node:test";
import {
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { GameServer, findRepoRoot } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

function findSavePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

/**
 * Turn a flat save containing two backpack weapons into the exact ownership
 * shape written by VR when it has one in each hand. This avoids simulating
 * motion controllers while still exercising the production deserializer and
 * MissionCore load path.
 */
function markWeaponsHeldByVr(
  savePath: string,
  leftEntity: number,
  rightEntity: number,
): void {
  const save = JSON.parse(readFileSync(savePath, "utf8"));
  const held = save.global_data.held_items;
  const inventoryLinks = held.held_entities.links[String(held.inventory_entity)];
  const heldIds = new Set([leftEntity, rightEntity]);

  inventoryLinks.to_links = inventoryLinks.to_links.filter(
    (link: { to_entity_id: number | null }) =>
      link.to_entity_id === null || !heldIds.has(link.to_entity_id),
  );
  held.entity_in_left_hand = leftEntity;
  held.entity_in_right_hand = rightEntity;
  held.held_entities.properties["P$HasRefs"][String(leftEntity)] = true;
  held.held_entities.properties["P$HasRefs"][String(rightEntity)] = true;

  writeFileSync(savePath, JSON.stringify(save));
}

test(
  "flat load holsters the displaced weapon from a VR two-hand save",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    const saveName = `cross_mode_held_${Date.now()}`;
    let savePath: string | undefined;
    t.after(() => {
      if (savePath) rmSync(savePath, { force: true });
    });

    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8530),
    });
    await game.step({ frames: 10 });

    const pistol = await game.player.spawnItem("Pistol");
    const shotgun = await game.player.spawnItem("Shotgun");
    await game.save(saveName);

    savePath = findSavePath(saveName);
    assert.ok(savePath, `save ${saveName} should exist on disk`);
    markWeaponsHeldByVr(savePath, pistol.entity_id, shotgun.entity_id);

    await game.load(saveName);
    await game.step({ frames: 2 });

    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.name === "Shotgun")?.location,
      "left_hand",
      `the second restored hand should become the flat viewmodel: ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      inventory.items.find((item) => item.name === "Pistol")?.location,
      "inventory",
      `the displaced first restored hand must return to the backpack: ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      inventory.items.filter((item) =>
        ["Pistol", "Shotgun"].includes(item.name ?? ""),
      ).length,
      2,
      "both VR-held weapons must remain reachable after the cross-mode load",
    );

    const loadedPistol = inventory.items.find((item) => item.name === "Pistol");
    assert.ok(loadedPistol);
    assert.equal(
      (await game.physics.bodies({ entityId: loadedPistol.entity_id })).bodies
        .length,
      0,
      "the holstered weapon must remain non-physical inside the backpack",
    );
  },
);
