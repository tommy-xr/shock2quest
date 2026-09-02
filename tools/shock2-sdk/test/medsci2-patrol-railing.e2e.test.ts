import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// End-to-end: medsci2's balcony patroller must not wedge on the railing.
//
// Its authored route includes a point on the floor BELOW the balcony, in a
// disconnected part of the navigation graph. With no route to it, the AI used
// to steer straight at the point, press into the converging corner behind a
// thin railing, and finally have its whole patrol retired by the stall
// watchdog - standing idle there for the rest of the mission. Zero player
// input reproduces it within a minute.
//
// Opt-in (needs Data/ assets + compiles the runtime): npm run test:e2e
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable across runs (runtime entity ids are not): the balcony patroller's
// template.
const PATROLLER_TEMPLATE = 668;

const SAMPLE_FRAMES = 60; // 1 s of simulation per sample
const SAMPLES = 60; // ...for a minute in total
// Holding within this distance (1 world unit = 2.5 Dark feet) counts as not
// moving.
const STUCK_RADIUS = 1.0;
// ...for this many consecutive samples (6 s) is a wedge.
const STUCK_SAMPLES = 6;

function aiProp(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

function distXZ(
  a: [number, number, number],
  b: [number, number, number],
): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

test(
  "the medsci2 balcony patroller never wedges on the railing",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci2.mis" });
    await game.step({ frames: 60 });

    const pipes = await game.entities.list({ filter: "OG-Pipe", limit: 200 });
    const patroller = pipes.entities.find(
      (entity) => entity.template_id === PATROLLER_TEMPLATE,
    );
    assert.ok(
      patroller,
      `medsci2 should contain the balcony patroller (template ${PATROLLER_TEMPLATE})`,
    );

    let anchor = (await game.entities.detail(patroller.id)).position;
    let heldSamples = 0;
    let worstHold = 0;
    let behavior: string | undefined;
    const wedges: string[] = [];

    for (let sample = 0; sample < SAMPLES; sample++) {
      await game.step({ frames: SAMPLE_FRAMES });
      const detail = await game.entities.detail(patroller.id);
      behavior = aiProp(detail, "AIBehavior");
      if (distXZ(detail.position, anchor) < STUCK_RADIUS) {
        heldSamples += 1;
        worstHold = Math.max(worstHold, heldSamples);
        if (heldSamples >= STUCK_SAMPLES) {
          wedges.push(
            `held ${heldSamples}s at ${detail.position
              .map((v) => v.toFixed(2))
              .join(",")} (behavior ${behavior ?? "?"})`,
          );
        }
      } else {
        heldSamples = 0;
        anchor = detail.position;
      }
    }

    assert.equal(
      wedges.length,
      0,
      `the patroller should never hold one spot for ${STUCK_SAMPLES}s: ${wedges.join("; ")}`,
    );
    assert.equal(
      behavior,
      "Patrol",
      `the patroller should still be patrolling after ${SAMPLES}s (worst hold ${worstHold}s)`,
    );
  },
);
