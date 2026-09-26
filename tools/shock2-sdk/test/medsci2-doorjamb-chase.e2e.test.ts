import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end: no medsci2 hybrid may grind in place while chasing.
//
// Chasing hybrids used to pile into the Science-wing door jamb: the first one
// wedged, and every follower inherited the identical route into the same spot
// because a stall inside a single cell reported nothing a re-path could route
// around. Every AI in the level chases at once here, so the pile-up is the
// thing under test, not one creature's luck.
//
// Opt-in (needs Data/ assets + compiles the runtime): npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SAMPLE_FRAMES = 60; // 1 s of simulation per sample
const SAMPLES = 30; // 1800 frames in total
// Holding within this distance (1 world unit = 2.5 Dark feet) counts as not
// moving...
const STUCK_RADIUS = 1.0;
// Holds are judged in the Science wing, north of this line - the corridor
// the chase runs up and where the pile-up happens. South of it is the
// balcony, whose separate wedge (a mesh route down a ledge the character
// controller cannot take) is the known-remaining defect and not what this
// test is about.
const SCIENCE_WING_Z = -60;
// ...and doing so for longer than this many seconds is a wedge.
const MAX_HOLD_SECONDS = 6;

function distXZ(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

test(
  "chasing medsci2 hybrids never grind in one spot",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.step({ frames: 60 });

    const pipes = await game.entities.list({ filter: "OG-Pipe", limit: 200 });
    assert.ok(
      pipes.entities.length > 0,
      "expected OG-Pipe hybrids in medsci2",
    );

    await game.input.trigger("DebugForceChase");

    // Longest run of consecutive samples each hybrid spent inside
    // STUCK_RADIUS of where that run started.
    const held = new Map<number, number>();
    const worst = new Map<number, number>();
    const anchor = new Map<number, [number, number, number]>();
    const lastAt = new Map<number, [number, number, number]>();
    // Hybrids that walked at least once. One that never moves at all has no
    // route to the player (medsci2's chase pass has several: locked areas,
    // other floors) - a grind is only meaningful for one that was walking.
    const mobile = new Set<number>();

    let sampled = 0;
    for (let i = 0; i < SAMPLES; i++) {
      await game.step({ frames: SAMPLE_FRAMES });
      // The hybrids chase a player who does not fight back, so the mission
      // ends when they reach him. Everything before that is the window
      // under test.
      if (!(await game.pathfinding.stats().catch(() => null))) break;
      sampled++;
      for (const pipe of pipes.entities) {
        const detail = await game.entities.detail(pipe.id).catch(() => null);
        // Dead or despawned: stop tracking it rather than freezing its run
        if (!detail) {
          held.set(pipe.id, 0);
          anchor.delete(pipe.id);
          continue;
        }
        const at = detail.position;
        lastAt.set(pipe.id, at);
        if (at[2] < SCIENCE_WING_Z) {
          // Out of the region under test: end any run it had going
          held.set(pipe.id, 0);
          anchor.delete(pipe.id);
          continue;
        }
        const from = anchor.get(pipe.id);
        if (from && distXZ(at, from) < STUCK_RADIUS) {
          const run = (held.get(pipe.id) ?? 0) + 1;
          held.set(pipe.id, run);
          worst.set(pipe.id, Math.max(worst.get(pipe.id) ?? 0, run));
        } else {
          if (from) mobile.add(pipe.id);
          held.set(pipe.id, 0);
          anchor.set(pipe.id, at);
        }
      }
    }

    assert.ok(
      sampled > MAX_HOLD_SECONDS + 4,
      `only ${sampled}s of chase to judge`,
    );

    const wedged = [...worst.entries()]
      .filter(([id, seconds]) => mobile.has(id) && seconds > MAX_HOLD_SECONDS)
      .map(
        ([id, seconds]) =>
          `${id} held ${seconds}s at ${JSON.stringify(lastAt.get(id))}`,
      );
    assert.deepEqual(
      wedged,
      [],
      `hybrids ground in place while chasing: ${wedged.join("; ")}`,
    );
  },
);

/** The hybrid that chases the player up the Science-wing corridor */
const HYBRID_TEMPLATE = 705;
/** Where it used to wedge: the door jamb beside the open security door */
const JAMB: [number, number, number] = [37.33, 0.1, -36.62];

// The pile-up test above judges every hybrid loosely; this one pins the
// specific route defect that agent-radius clearance fixes - the nav mesh
// threads a 0.6-wide strip beside the jamb that a 0.96-wide hybrid cannot
// walk, and the chaser used to press into that corner and grind.
test(
  "a chasing hybrid takes the open door, not the sliver beside its jamb",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.step({ frames: 60 });

    const [hybrid] = await game.entities.byTemplate(HYBRID_TEMPLATE);
    assert.ok(
      hybrid,
      `expected a hybrid of template ${HYBRID_TEMPLATE} in medsci2`,
    );

    await game.input.trigger("DebugForceChase");

    const track: [number, number, number][] = [];
    for (let i = 0; i < 20; i++) {
      await game.step({ frames: 60 });
      const detail = await game.entities.detail(hybrid.id).catch(() => null);
      if (!detail) break;
      track.push(detail.position);
    }
    assert.ok(
      track.length >= 10,
      `hybrid vanished after ${track.length} samples`,
    );

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
    assert.ok(
      travelled > 8,
      `hybrid barely moved (${travelled.toFixed(1)}): ${JSON.stringify(track)}`,
    );
  },
);
