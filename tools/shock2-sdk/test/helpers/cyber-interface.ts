import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";

/** Earth's authored ChangeInterface marker (the training entrance's trap). */
const CHANGE_INTERFACE_TEMPLATE = 328;

/**
 * Install the cyber interface on a fresh Earth launch, which (like retail)
 * starts without it. Drives the authored ChangeInterface script rather than a
 * test-only quest override; tests that cover installation itself should cross
 * the real training tripwire instead.
 */
export async function installEarthCyberInterface(game: GameServer): Promise<void> {
  const markers = await game.entities.byTemplate(CHANGE_INTERFACE_TEMPLATE);
  assert.equal(markers.length, 1, "expected exactly one Earth ChangeInterface marker");
  await game.entities.sendMessage(markers[0]!.id, { type: "TurnOn" });
  await game.step({ frames: 1 });
}
