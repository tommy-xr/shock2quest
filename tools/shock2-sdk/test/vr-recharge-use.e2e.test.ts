import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("VR: free-hand trigger recharges carried cells without an access card", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  const cells = (await game.entities.list({ filter: "Dead Power Cell", limit: 20 })).entities;
  assert.ok(cells.length > 0);
  for (const cell of cells) await game.player.give(cell.id);
  assert.equal((await game.player.inventory()).items.filter(i => i.name === "Dead Power Cell").length, cells.length);

  const [station] = await game.entities.byTemplate(506);
  assert.equal(station?.name, "Recharging Station");
  await game.player.teleport({ x: station.position[0] + 3, y: station.position[1], z: station.position[2] });
  await game.step({ frames: 5 });
  await aimVrHandAt(game, station.position, 1.5);
  // An empty hand hovering cannot charge backpack items. The trigger must do it.
  assert.equal((await game.player.inventory()).items.filter(i => i.name === "Dead Power Cell").length, cells.length);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 10 });
  await game.input.set("right_hand.trigger", 0);
  const messages = (await game.ui.state()).messages;
  assert.ok(!messages.some(m => m.includes("Scan your personal access card")), JSON.stringify(messages));
  const inventory = (await game.player.inventory()).items;
  assert.equal(inventory.filter(i => i.name === "Dead Power Cell").length, 0);
  assert.equal(inventory.filter(i => i.name === "Power Cell").length, cells.length);
});
