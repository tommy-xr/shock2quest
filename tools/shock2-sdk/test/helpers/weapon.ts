import assert from "node:assert/strict";

import type { EntitySummary, GameServer } from "../../src/index.js";

/** The `Ammo` property of a weapon's entity detail - its loaded rounds. */
export function ammoOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const ammo = detail.properties.find((p) => p.name === "Ammo");
  assert.ok(ammo, "weapon should expose an Ammo property");
  return Number(ammo.value);
}

/** Fire one round: edge-triggered pull (fires on the rising edge), then release,
 * stepping a frame for each so the next pull is a fresh edge. */
export async function fireOnce(game: GameServer): Promise<void> {
  await game.input.set("right_hand.trigger", 1.0);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0.0);
  await game.step({ frames: 1 });
}

/** Trigger `DebugCycleWeapon` until it spawns an entity matching `match`, and
 * return that freshly created entity. The lookup is diffed against the entity
 * list before each trigger: the `debug_weapons` bench stocks one of every gun,
 * so a by-name or by-template search over the whole scene is ambiguous.
 * Debug-scene scale only: /v1/entities sorts by distance and truncates at the
 * limit, so in a large mission a far-away spawn could fall off the list. */
export async function cycleToWeapon(
  game: GameServer,
  match: (e: EntitySummary) => boolean,
  { cycles = 16, settleFrames = 10 }: { cycles?: number; settleFrames?: number } = {},
): Promise<EntitySummary> {
  for (let i = 0; i < cycles; i += 1) {
    const before = new Set(
      (await game.entities.list({ limit: 300 })).entities.map((e) => e.id),
    );
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: settleFrames });
    const spawned = (await game.entities.list({ limit: 300 })).entities.find(
      (e) => !before.has(e.id) && match(e),
    );
    if (spawned) return spawned;
  }
  throw new Error("DebugCycleWeapon did not spawn a matching weapon");
}
