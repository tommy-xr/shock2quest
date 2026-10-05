import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";

// #1919: RepBase has no PhysType. Its selection box pushed purchases into
// the alcove wall. Provision currency; buy, crouch and pick up through input.
export async function verifyManyReplicatorDelivery(game: GameServer): Promise<void> {
  await game.player.spawnItem("20 Nanites");
  await game.player.spawnItem("20 Nanites");
  await game.step({ frames: 5 });
  const [replicator] = await game.entities.byTemplate(56);
  const [marker] = await game.entities.byTemplate(485);
  assert.ok(replicator);
  assert.ok(marker);
  const before = new Set((await game.entities.byTemplate(-53)).map(e => e.id));
  await game.player.teleport({ x: -43.33539, y: 2.644, z: -55.28013 });
  await game.player.aimAt(replicator.id, { hitbox: "center", visibility: "required" });
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 1 });
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel);
  assert.equal(panel.template_id, 56, "physical frob opens the authored replicator");
  const buy = panel.elements.find(e => e.label === "buy:detox patch");
  assert.ok(buy);
  await clickUiElement(game, buy);
  const item = (await game.entities.byTemplate(-53)).find(e => !before.has(e.id));
  assert.ok(item, "35 nanites should dispense a detox patch");
  const spawned = (await game.entities.detail(item.id)).position;
  assert.ok(Math.hypot(...spawned.map((v, i) => v - marker.position[i])) < 1,
    "output still originates at the authored hopper marker");
  const close = (await game.ui.state()).active_panel?.elements.find(e => e.label === "close");
  assert.ok(close);
  await clickUiElement(game, close);
  await game.step({ frames: 300 });
  await game.input.set("crouch", 1);
  await game.step({ frames: 10 });
  const detail = await game.entities.detail(item.id);
  assert.ok(detail.selection_bounds);
  const [lo, hi] = detail.selection_bounds;
  // Aim at the exposed end on the floor. The hypo's center is still behind
  // RepBase's broad selection box, which makes center-only aiming unsuitable.
  const target: [number, number, number] = [
    lo[0] + 0.2 * (hi[0] - lo[0]),
    lo[1] + 0.3 * (hi[1] - lo[1]),
    lo[2] + 0.75 * (hi[2] - lo[2]),
  ];
  const info = await game.info();
  const eye: [number, number, number] = [...info.player.position];
  eye[1] += info.player.camera_offset?.[1] ?? 0.48;
  const hit = await game.raycast({
    start: eye, end: target,
    collision_groups: ["entity", "selectable", "world", "ui", "raycast"],
    ignore_sensors: true,
  });
  assert.equal(hit.entity_id, item.id, "the purchase must be visible from the shop front");
  await game.input.lookAtWorldPoint(target);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 1 });
  assert.ok((await game.player.inventory()).items.some(e => e.entity_id === item.id),
    "ordinary squeeze must collect the purchase");
}

test("Many replicator dispenses a physically collectible purchase", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 300_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "many.mis" });
  await verifyManyReplicatorDelivery(game);
});
