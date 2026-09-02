import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The hybrid that chases the player up the Science-wing corridor */
const HYBRID_TEMPLATE = 705;
/** Where it used to wedge: the door jamb beside the open security door */
const JAMB: [number, number, number] = [37.33, 0.1, -36.62];

function distXZ(a: [number, number, number], b: [number, number, number]): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

test(
  "a chasing hybrid takes the open door, not the sliver beside its jamb",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.step({ frames: 60 });

    const [hybrid] = await game.entities.byTemplate(HYBRID_TEMPLATE);
    assert.ok(hybrid, `expected a hybrid of template ${HYBRID_TEMPLATE} in medsci2`);

    await game.input.trigger("DebugForceChase");

    // Sample its position each second of simulation. The nav mesh threads a
    // 0.6-wide strip beside the jamb, which a 0.96-wide hybrid cannot walk:
    // it used to press into the corner there and grind for the rest of the
    // run.
    const track: [number, number, number][] = [];
    for (let i = 0; i < 20; i++) {
      await game.step({ frames: 60 });
      const detail = await game.entities.detail(hybrid.id).catch(() => null);
      if (!detail) break;
      track.push(detail.position);
    }
    assert.ok(track.length >= 10, `hybrid vanished after ${track.length} samples`);

    let pinnedAtJamb = 0;
    let worstPinnedAtJamb = 0;
    for (const at of track) {
      pinnedAtJamb = distXZ(at, JAMB) < 1.0 ? pinnedAtJamb + 1 : 0;
      worstPinnedAtJamb = Math.max(worstPinnedAtJamb, pinnedAtJamb);
    }
    assert.ok(
      worstPinnedAtJamb < 4,
      `hybrid sat at the door jamb for ${worstPinnedAtJamb}s: ${JSON.stringify(track)}`,
    );

    // ...and it got somewhere: the chase runs the length of the corridor,
    // well past the door it was stuck outside of.
    const travelled = distXZ(track[0], track[track.length - 1]);
    assert.ok(travelled > 8, `hybrid barely moved (${travelled.toFixed(1)}): ${JSON.stringify(track)}`);
  },
);
