import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { add, sub, dot } from "./helpers/vr-hand.js";
import { litFraction } from "./helpers/screenshot-pixels.js";

// The VR forearm HUD has to be photographable from the offscreen `--vr`
// runtime: PRs that added forearm panels could not capture them because the
// panels, while drawn, sat *above* the eye and so outside the frame - a black
// screenshot that read as "VR renders nothing offscreen".
//
// Both presentations park their hands somewhere by default (the debug runtime
// for a mission, the `debug_hud` scene for itself), so assert the same thing
// about both: the hands are within reach BELOW the eye, where a flat-on-the-arm
// panel can be seen at all. The exact frustum containment is asserted in
// `debug_runtime`'s own unit test, which knows the projection; here the check
// is the invariant that broke, plus - for `debug_hud`, whose only content is
// the panels - that the capture is not black.
//
// Negative-first: with the previous default poses (hands 0.36 world units ABOVE
// the eye) both the eye-relative check and the lit-frame check fail.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Arm's length in world units - 1 unit is 2.5 SS2 feet. */
const MAX_HAND_REACH = 1.2;

async function assertHandsAreWhereThePanelsCanBeSeen(
  game: GameServer,
  label: string,
) {
  const { player } = await game.info();
  assert.ok(player, `${label}: expected a player`);
  const eye = add(player.position, player.camera_offset);

  const hands = await game.scene.fromSource("player_hands");
  assert.ok(
    hands.length > 0,
    `${label}: expected the VR hands/forearm panels to be drawn`,
  );

  for (const object of hands) {
    const fromEye = sub(object.position, eye);
    assert.ok(
      fromEye[1] < 0,
      `${label}: hand object sits above the eye, where its panel faces away (${JSON.stringify(object.position)} vs eye ${JSON.stringify(eye)})`,
    );
    const distance = Math.sqrt(dot(fromEye, fromEye));
    assert.ok(
      distance < MAX_HAND_REACH,
      `${label}: hand object is ${distance.toFixed(2)} units from the eye, beyond arm's reach`,
    );
  }
}

test(
  "the debug_hud scene photographs its forearm panels",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_hud",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });
    await assertHandsAreWhereThePanelsCanBeSeen(game, "debug_hud --vr");

    // This scene draws nothing but the panels, so the frame being black is
    // exactly the bug and the lit area is exactly the panels.
    const shot = await game.screenshot("vr-forearm-panels-debug-hud.png");
    const lit = litFraction(shot.full_path);
    assert.ok(
      lit > 0.02,
      `debug_hud --vr captured an essentially black frame (${(lit * 100).toFixed(2)}% lit)`,
    );
  },
);

test(
  "a mission's default --vr hand pose keeps the forearm panels in view",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await assertHandsAreWhereThePanelsCanBeSeen(game, "medsci1 --vr");
    await game.screenshot("vr-forearm-panels-medsci1.png");
  },
);
