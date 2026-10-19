import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

// Heights of the game's hands and a thigh holster relative to the rendered eye.
async function belowEye(game: GameServer) {
  const player = (await game.info()).player;
  const eye = player.position[1] + player.camera_offset[1];
  const [left, right] = player.hand_feedback?.glove_contacts?.centers ?? [null, null];
  const holster = player.hand_feedback?.holsters?.centers?.[0];
  assert.ok(left && right && holster);
  return { eye, left: left[1] - eye, right: right[1] - eye, holster: holster[1] - eye };
}

test("a VR stick crouch lowers the hands and the belt with the eye", { skip: !enabled, timeout: 120_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const standing = await belowEye(game);

  await game.input.set("crouch", 1);
  await game.step({ frames: 30 });
  const crouched = await belowEye(game);

  assert.ok(standing.eye - crouched.eye > 0.5, `the eye drops: ${standing.eye} -> ${crouched.eye}`);
  for (const part of ["left", "right", "holster"] as const) {
    assert.ok(
      Math.abs(crouched[part] - standing[part]) < 0.02,
      `${part} keeps its place below the eye: ${standing[part]} -> ${crouched[part]}`,
    );
  }

  // A world target still lands on its point from the crouch.
  const player = (await game.info()).player;
  const target: [number, number, number] = [player.position[0], player.position[1] + player.camera_offset[1] - 0.4, player.position[2] + 0.4];
  await game.input.set("right_hand.world_target", target);
  await game.step({ frames: 5 });
  const world = (await game.input.state()).right_hand.world_position!;
  assert.ok(Math.hypot(...world.map((v, i) => v - target[i])) < 0.02, `hand ${world} on ${target}`);
});
