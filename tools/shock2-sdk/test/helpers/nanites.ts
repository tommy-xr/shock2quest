import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";

/**
 * Read the authored `StackCount` property off an entity's `properties` list
 * (as returned by `game.entities.detail`), if present.
 */
export function stackCount(
  properties: { name: string; value: string }[],
): number | undefined {
  const stack = properties.find((property) => property.name === "StackCount");
  return stack === undefined ? undefined : Number(stack.value);
}

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
    const stack = stackCount(detail.properties);
    assert.ok(stack !== undefined, `carried nanite entity ${item.entity_id} needs StackCount`);
    total += stack;
  }
  return total;
}
