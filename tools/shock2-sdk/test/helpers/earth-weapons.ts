import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";
import type { EntitySummary } from "../../src/types.js";
import { earthWorldUse } from "./earth-world-use.js";

export interface EarthWeaponsPickup {
  pistol: EntitySummary;
  clips: EntitySummary[];
}

/** Pick up the authored Earth Weapons Training pistol and small standard clips
 * through the normal flat crosshair + squeeze path. Runtime ids are discovered
 * each launch; the positive template ids are the stable mission object ids. */
export async function pickupEarthWeapons(
  game: GameServer,
  clipCount: number,
): Promise<EarthWeaponsPickup> {
  const entities = (await game.entities.list()).entities;
  const pistol = entities.find(
    (entity) => entity.template_id === 246 && entity.name === "Pistol",
  );
  const clips = entities
    .filter(
      (entity) =>
        entity.template_id >= 247 &&
        entity.template_id <= 251 &&
        entity.name.includes("Standard Clip"),
    )
    .sort((a, b) => a.template_id - b.template_id)
    .slice(0, clipCount);
  assert.ok(pistol, "expected Earth Weapons Training pistol 246");
  assert.equal(clips.length, clipCount, `expected ${clipCount} Earth small standard clips`);

  await earthWorldUse(game, pistol);
  assert.equal(
    (await game.info()).player.wielded_entity_id,
    pistol.id,
    "normal world-use should wield the Earth pistol",
  );
  for (const clip of clips) {
    await earthWorldUse(game, clip);
  }

  return { pistol, clips };
}
