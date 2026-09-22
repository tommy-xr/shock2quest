import assert from "node:assert/strict";
import type { GameServer } from "../../src/index.js";

/** Drive the court lamps bidirectionally. Button 77 is a one-way activator;
 * its seventeenth link is a one-shot quest-bit setter, not a lamp. Discover
 * live IDs on every call so this also works after a save/load. */
export async function switchCourtLights(game: GameServer, type: "TurnOn" | "TurnOff") {
  const [button] = await game.entities.byTemplate(77);
  assert.ok(button, "authored court-light switch");
  const lamps = (await game.entities.detail(button.id)).outgoing_links
    .filter(link => link.link_type === "SwitchLink" && /light/i.test(link.target_name));
  assert.equal(lamps.length, 16, "authored lamp targets, excluding quest-bit setter");
  for (const lamp of lamps) await game.entities.sendMessage(lamp.target_id, { type });
  await game.step({ frames: 2 });
}
