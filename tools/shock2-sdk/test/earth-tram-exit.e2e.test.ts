import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the earth.mis intro: the player must be able to WALK out
// of the UNN tram they spawn in, across the station boardwalk, to the base of
// the recruitment-center gravshafts (the level's first traversal).
//
// Negative-first: the player rests on the tram's floor slab - a yaw-ROTATED
// kinematic cuboid - at exactly the character controller's contact offset.
// Without the grounded rest lift (see `PLAYER_REST_LIFT` in
// shock2vr/src/physics/mod.rs), every movement cast re-hits that resting
// contact at toi=0 (the rotated face normal's ~1e-6 tilt makes tangential
// walking read as "approaching"), the controller applies zero translation,
// and the player freezes inside the tram after ~one frame of walking - this
// walk never gets past the tram doorway (z stays ~-11).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "earth.mis: the player can walk out of the intro tram to the gravshaft base",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8112),
    });
    await game.step({ frames: 30 }); // settle onto the tram floor

    const spawn = await game.player.position();
    assert.ok(
      spawn.z < -10,
      `expected to spawn inside the tram (z < -10), got z=${spawn.z}`,
    );

    // Hold forward: out the tram door, across the boardwalk, until the wall
    // at the gravshaft base stops the walk (~z=12.4).
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 300 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const end = await game.player.position();
    assert.ok(
      end.z > 10,
      `walking forward from the tram should reach the gravshaft base ` +
        `(z > 10), got z=${end.z.toFixed(2)} (frozen inside the tram means ` +
        `the resting-contact fix regressed - see PLAYER_REST_LIFT)`,
    );
  },
);
