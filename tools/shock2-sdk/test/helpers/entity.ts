import assert from "node:assert/strict";

import type { EntityDetailResult } from "../../src/index.js";

/** An entity's current `HitPoints` property. */
export function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find((p) => p.name === "HitPoints");
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}
