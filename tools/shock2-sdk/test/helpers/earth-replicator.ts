import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";
import type { EntitySummary } from "../../src/types.js";
import { teleportVerified } from "./teleport.js";

/**
 * The player's total spendable nanite balance: the persistent stat balance
 * (world nanite pickups collect straight into it, never inventory) plus any
 * legacy carried nanite StackCount entities exposed through inventory
 * (pre-existing saves, panel-taken piles). Mirrors
 * `script_util::player_nanite_total` on the Rust side.
 */
export async function carriedNaniteTotal(game: GameServer): Promise<number> {
  const stat = (await game.info()).player.stats?.nanites ?? 0;
  const inventory = await game.player.inventory();
  let total = stat;
  for (const item of inventory.items) {
    if (!item.name?.toLowerCase().includes("nanite")) continue;
    const detail = await game.entities.detail(item.entity_id);
    const stack = detail.properties.find(
      (property) => property.name === "StackCount",
    );
    assert.ok(stack, `carried nanite entity ${item.entity_id} needs StackCount`);
    total += Number(stack.value);
  }
  return total;
}

/**
 * Stage at Earth Technical Training's clear standing point and physically frob
 * the authored replicator through the normal crosshair/squeeze path.
 */
export async function physicallyOpenEarthReplicator(
  game: GameServer,
  replicator: EntitySummary,
): Promise<void> {
  const [x, _y, z] = (await game.entities.detail(replicator.id)).position;
  await teleportVerified(game, { x: x - 1.59, y: 21.404, z: z - 2.23 });
  await game.input.set("head.look", [-35.3, -22]);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
}
