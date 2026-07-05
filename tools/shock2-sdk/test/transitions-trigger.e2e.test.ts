import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for trigger-based level chaining. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before GET /v1/transitions, a tester could not discover where
// a level's transition triggers lead at runtime (DestLevel is an inherited
// property, invisible in entity-detail), so the only way to change level was the
// explicit transitionLevel() warp. This discovers the forward trigger, teleports
// into its volume, and lets the REAL trigger script fire the transition.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "transitions: discover the forward trigger and let it fire the level change",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8106),
    });
    await game.step({ frames: 5 });

    const { transitions } = await game.transitions();
    assert.ok(transitions.length > 0, "medsci1 should expose transition triggers");

    // Find the forward trigger to Engineering.
    const toEng1 = transitions.find((t) => t.dest_level === "eng1");
    assert.ok(toEng1, `expected a trigger to eng1, got ${JSON.stringify(transitions.map((t) => t.dest_level))}`);
    assert.ok(
      toEng1.position.every(Number.isFinite),
      "the trigger should have a finite volume position",
    );

    // Teleport into the trigger's volume and step - the real TrapTripLevel
    // script fires on SensorBeginIntersect (no explicit warp).
    const [x, y, z] = toEng1.position;
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 15 });

    assert.equal(
      (await game.info()).mission,
      "eng1.mis",
      "entering the trigger volume should transition to eng1 via the real trigger",
    );
  },
);
