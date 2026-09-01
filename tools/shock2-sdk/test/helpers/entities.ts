import assert from "node:assert/strict";

import type {
  EntityDetailResult,
  EntitySummary,
  GameServer,
} from "../../src/index.js";

/** One introspected property of an entity, by name. */
export function property(
  detail: EntityDetailResult,
  name: string,
): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

/**
 * The single entity of a template, asserting there is exactly one. Runtime
 * entity ids are not stable across runs; templates are, so scenes are addressed
 * this way.
 */
export async function only(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(
    matches.length,
    1,
    `${label}: expected one, got ${JSON.stringify(matches)}`,
  );
  return matches[0]!;
}
