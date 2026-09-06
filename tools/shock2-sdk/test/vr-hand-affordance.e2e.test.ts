import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The VR glove's light and finger pre-shape are driven by one resolved
// affordance per hand, read off the same raycast the trigger and squeeze act
// on. This checks the readout that state reports (`player.hand_affordance`):
// a pickup under the hand's ray is grabbable, and a hand pointing at nothing
// eventually goes dark.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "VR: a hand aimed at a pickup reads Grabbable, and at nothing reads None",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    // DebugCycleWeapon spawns the pistol; VR wield is a no-op so it drops to
    // the floor in front of the player.
    await game.step({ frames: 10 });
    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol", {
      settleFrames: 90,
    });

    // Park the right hand on the pistol's forward raycast axis (debug_weapons
    // spawns the pawn at the origin with identity rotation, so pawn-local ==
    // world minus the pawn position). No squeeze: the point is the hover.
    const pawnY = (await game.info()).player.position[1];
    const [px, py, pz] = pistol.position;
    await game.input.set("right_hand.position", [px + 0.4, py - pawnY, pz]);
    await game.step({ frames: 10 });

    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "Grabbable",
      "a pickup under the hand's ray should light the glove for a grab",
    );

    // Aim the same hand straight up ([x,y,z,w] for +90 degrees about X takes
    // the hand's -Z forward to +Y), at open air. The hover survives a few
    // frames of hysteresis, so step well past it.
    await game.input.set("right_hand.rotation", [0.7071068, 0, 0, 0.7071068]);
    await game.step({ frames: 30 });

    assert.equal(
      (await game.info()).player.hand_affordance.right,
      "None",
      "a hand pointing at nothing should go dark",
    );
  },
);
