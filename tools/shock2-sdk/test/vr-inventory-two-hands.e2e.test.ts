import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { canvasCenter, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000 };

test("VR inventory lets both hands take and move items without duplicating ownership", options, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const left = (await game.player.spawnItem("Maintenance Tool")).entity_id;
  const right = (await game.player.spawnItem("Med Patch")).entity_id;
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  const ui = await game.ui.state();
  const panel = requirePanelPose(ui);
  const slot = (id: number) => {
    const element = ui.strip!.elements.find(e => e.kind === "button" && e.entity_id === id);
    assert.ok(element);
    return canvasCenter(element);
  };
  await aimVrHandAtCanvas(game, panel, slot(left), { hand: "left", squeeze: 0 });
  await aimVrHandAtCanvas(game, panel, slot(right), { hand: "right", squeeze: 0 });
  await game.step({ frames: 3 });
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });
  let player = (await game.info()).player;
  assert.equal(player.wielded_entity_id, left);
  assert.equal(player.right_hand_entity_id, right);

  // Two independent destination previews, then release into different cells.
  const destinations: [number, number][] = [[140, 50], [205, 50]];
  await aimVrHandAtCanvas(game, panel, destinations[0], { hand: "left", squeeze: 1 });
  await aimVrHandAtCanvas(game, panel, destinations[1], { hand: "right", squeeze: 1 });
  await game.step({ frames: 3 });
  const previews = (await game.ui.state()).strip!.elements.filter(e => e.kind === "image" && e.label?.includes("RELEASE"));
  assert.equal(previews.length, 2, "both hands should show placement previews");
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  player = (await game.info()).player;
  assert.equal(player.wielded_entity_id, null);
  assert.equal(player.right_hand_entity_id, null);
  const placed = await game.ui.state();
  for (const [id, destination] of [[left, destinations[0]], [right, destinations[1]]] as const) {
    const element = placed.strip!.elements.find(e => e.entity_id === id && e.kind === "button");
    assert.ok(element);
    const [x, y, w, h] = element.rect;
    assert.ok(destination[0] >= x && destination[0] <= x + w && destination[1] >= y && destination[1] <= y + h);
  }

  // Simultaneous grabs of one slot must result in exactly one owner.
  const target = canvasCenter(placed.strip!.elements.find(e => e.entity_id === left && e.kind === "button")!);
  await aimVrHandAtCanvas(game, panel, target, { hand: "left", squeeze: 0 });
  await aimVrHandAtCanvas(game, panel, target, { hand: "right", squeeze: 0 });
  await game.step({ frames: 3 });
  await game.input.set("left_hand.squeeze", 1);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });
  player = (await game.info()).player;
  assert.equal([player.wielded_entity_id, player.right_hand_entity_id].filter(id => id === left).length, 1);
  assert.equal((await game.player.inventory()).items.filter(item => item.entity_id === left).length, 1);
});
