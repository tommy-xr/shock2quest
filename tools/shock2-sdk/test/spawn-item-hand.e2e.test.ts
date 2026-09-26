import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("spawn-item with a hand grabs the item into that VR hand", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: ["--vr"] });
  await game.step({ frames: 10 });
  // A VR hand releases what it holds when the grip lets go, so hold it.
  await game.input.set("right_hand.squeeze", 1);
  await game.input.set("left_hand.squeeze", 1);
  const pistol = await game.player.spawnItem("Pistol", { hand: "right" });
  const amp = await game.player.spawnItem(-247, { hand: "left" });
  await game.step({ frames: 2 });
  const { player } = await game.info();
  assert.equal(player.right_hand_entity_id, pistol.entity_id);
  // In VR, wielded_entity_id is the left hand's slot.
  assert.equal(player.wielded_entity_id, amp.entity_id);
});
