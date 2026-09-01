import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The VR forearm HUD has to be photographable from the offscreen `--vr`
// runtime: PRs that added forearm panels could not capture them because the
// panels, while drawn, sat *above* the eye and outside the frame - a black
// screenshot that looked like "VR renders nothing offscreen".
//
// Both presentations park their hands somewhere by default (the debug runtime
// for a mission, the `debug_hud` scene for itself), so assert the same thing
// about both: what `/v1/scene` reports as `player_hands` lands inside the
// picture the runtime renders.
//
// Negative-first: with the previous default poses (hands 0.36 world units
// ABOVE the eye) every one of these objects is behind/above the frustum and
// the frame check fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The runtime's projection: `shock2vr::DEFAULT_FOV_DEG` vertical over its 4:3
// framebuffer.
const HALF_FOV_V_DEG = 45 / 2;
const HALF_FOV_H_DEG =
  (Math.atan(Math.tan((HALF_FOV_V_DEG * Math.PI) / 180) * (4 / 3)) * 180) /
  Math.PI;

/** Assert every hand-path object this frame is inside the rendered frustum. */
async function assertHandObjectsAreInFrame(game: GameServer, label: string) {
  const info = await game.info();
  const player = info.player;
  assert.ok(player, `${label}: expected a player`);
  const eye: [number, number, number] = [
    player.position[0] + player.camera_offset[0],
    player.position[1] + player.camera_offset[1],
    player.position[2] + player.camera_offset[2],
  ];

  const hands = await game.scene.fromSource("player_hands");
  assert.ok(
    hands.length > 0,
    `${label}: expected the VR hands/forearm panels to be drawn`,
  );

  for (const object of hands) {
    // Pawn forward is -X at the default (unrotated) camera; the lateral axis
    // is Z. Both scenes under test face that way.
    const forward = eye[0] - object.position[0];
    const down = eye[1] - object.position[1];
    const across = object.position[2] - eye[2];
    assert.ok(
      forward > 0,
      `${label}: hand object is behind the eye (${JSON.stringify(object.position)})`,
    );
    const downDeg = (Math.atan(down / forward) * 180) / Math.PI;
    const acrossDeg = (Math.atan(Math.abs(across) / forward) * 180) / Math.PI;
    assert.ok(
      Math.abs(downDeg) < HALF_FOV_V_DEG,
      `${label}: hand object is outside the vertical FOV (${downDeg.toFixed(1)} deg)`,
    );
    assert.ok(
      acrossDeg < HALF_FOV_H_DEG,
      `${label}: hand object is outside the horizontal FOV (${acrossDeg.toFixed(1)} deg)`,
    );
  }
}

test(
  "the debug_hud scene puts its forearm panels in shot",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_hud",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });
    await assertHandObjectsAreInFrame(game, "debug_hud --vr");
    await game.screenshot("vr-forearm-panels-debug-hud.png");
  },
);

test(
  "a mission's default --vr hand pose puts the forearm panels in shot",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await assertHandObjectsAreInFrame(game, "medsci1 --vr");
    await game.screenshot("vr-forearm-panels-medsci1.png");
  },
);
