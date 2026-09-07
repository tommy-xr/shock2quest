import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The finger fit (`shock2vr::hand_fit`) solves each finger's curl against the
// held item's own render mesh at grab time. `/v1/info` reports the answer per
// hand, which is the only observable the fit has - the pose itself lives in
// skinning matrices no endpoint reports.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "VR: a held pistol fits a trigger grip, index trailing the wrapping fingers",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });

    await game.step({ frames: 10 });

    // An empty hand has nothing to fit.
    assert.equal((await game.info()).player.hand_affordance.right_grip, null);

    // DebugCycleWeapon spawns the pistol; the VR wield is a no-op so it drops
    // to the floor in front of the player.
    const pistol = await cycleToWeapon(game, (e) => e.name === "Pistol", {
      settleFrames: 90,
    });

    // Grab it, the same way vr-held-model does: park the right hand on the
    // pistol's forward raycast axis and squeeze.
    const pawnY = (await game.info()).player.position[1];
    const [px, py, pz] = pistol.position;
    await game.input.set("right_hand.position", [px + 0.4, py - pawnY, pz]);
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol.id,
      "pistol should be grabbed by the right hand",
    );

    const grip = (await game.info()).player.hand_affordance.right_grip;
    assert.ok(grip, "a held pistol should report a fitted grip");

    // A gun is held by name, not by measurement: the index rests on the
    // trigger while the rest of the hand wraps the grip. Without the fit every
    // finger took one hardcoded curl and this ordering was an accident of
    // those constants rather than a property of the weapon.
    assert.equal(grip.family, "trigger");
    for (const finger of ["middle", "ring", "pinky"] as const) {
      assert.ok(
        grip.index < grip[finger],
        `index (${grip.index}) should trail ${finger} (${grip[finger]})`,
      );
    }
    // Every curl is a blend amount, and the wrapping fingers really close.
    for (const finger of ["thumb", "index", "middle", "ring", "pinky"] as const) {
      assert.ok(
        grip[finger] >= 0 && grip[finger] <= 1,
        `${finger} curl out of range: ${grip[finger]}`,
      );
    }
    assert.ok(grip.middle > 0.5, `middle should wrap the grip: ${grip.middle}`);

    // Dropping it takes the fit away again.
    await game.input.set("right_hand.squeeze", 0.0);
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.hand_affordance.right_grip, null);
  },
);
