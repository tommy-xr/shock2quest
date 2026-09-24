import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// #1408: the left column of Earth Technical's keypad HACK board extended
// behind the surrounding wall trim despite its center being unobstructed.
test("Earth VR keypad exposes the full board ahead of wall trim", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 120_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", debugFlags: ["--vr"] });
  await game.step({ frames: 3 });
  const [keypad] = await game.entities.byTemplate(266);
  assert.ok(keypad, "authored Technical Training keypad must exist");
  await teleportVerified(game, { x: 172.56207, y: 22.044, z: 175.5225 });
  await aimVrHandAt(game, [172.38925, 22.8, 174.05382], 0.5);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  const panels = (await game.physics.bodies()).bodies.filter(body => body.collision_groups.includes("ui"));
  assert.equal(panels.length, 1, "real controller trigger opens the keypad board");
  const panel = panels[0];
  const target: Vec3 = [172.10925, 23.024, panel.position[2]];
  const aim = await aimVrHandAt(game, target, 0.6);
  const hit = await game.raycast({
    start: aim.start, end: target,
    collision_groups: ["ui", "world", "entity", "selectable", "raycast"],
    ignore_sensors: true,
  });
  assert.equal(hit.entity_id, panel.entity_id, "left node must be in front of world trim on the ordinary controller ray");
  await game.step({ frames: 30 });
  const later = (await game.physics.bodies()).bodies.find(body => body.entity_id === panel.entity_id);
  assert.deepEqual(later?.position, panel.position, "existing proxy must not push its own panel outward on later updates");
});
