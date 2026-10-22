import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("VR: a chemical's Collect hint matches its trigger action", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const [chemical] = await game.entities.byTemplate(-20);
  assert.ok(chemical, "the research lab has Fermium");
  await game.player.teleport({ x: chemical.position[0], y: 1.25, z: 0 });
  await game.step({ frames: 2 });
  await aimVrHandAt(game, chemical.position, 0.2);
  assert.ok((await game.ui.state()).bracket_hints.includes("Collect"));
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 3 });
  assert.ok((await game.player.inventory()).items.some(i => i.entity_id === chemical.id));
  assert.ok(!(await game.ui.state()).bracket_hints.includes("Collect"), "the carried item no longer offers collection");
});
