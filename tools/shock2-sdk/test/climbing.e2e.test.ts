import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for flat (desktop-style) ladder climbing.
//
// medsci1's critical path starts with a REQUIRED ladder climb out of the cryo
// recovery bay (rung entities named "Rick Ladder", PropPhysAttr.climbable=27,
// stacked at x=-41.3, z=16.5). A player pressing forward into the ladder must
// ascend it - Half-Life style - instead of being stopped by the ladder's
// collider.
//
// Negative-first: without the climbing implementation the forward input just
// presses the player against the ladder collider (y stays at floor height), so
// the ascent assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "flat climbing: pushing into a ladder ascends it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    // Resolve the cryo-recovery ladder at run time (runtime entity ids are not
    // stable across launches): cluster the "Rick Ladder" rungs by column and
    // take the column near (-17.5, 14.5) - the shaft ladder on medsci1's
    // critical path. (The other cryo-bay column, at (-41.3, 16.5), is fenced
    // off by the fallen air duct the player is meant to wrench-smash first, so
    // it can't be walked up to.)
    const rungs = (await game.entities.list({ filter: "Rick Ladder", limit: 100 }))
      .entities;
    assert.ok(rungs.length > 0, "medsci1 should contain Rick Ladder entities");

    const columns = new Map<string, { x: number; z: number; ys: number[] }>();
    for (const rung of rungs) {
      const [x, y, z] = rung.position;
      const key = `${Math.round(x)},${Math.round(z)}`;
      const col = columns.get(key) ?? { x, z, ys: [] };
      col.ys.push(y);
      columns.set(key, col);
    }
    const ladder = [...columns.values()].reduce((best, col) => {
      const d = (c: { x: number; z: number }) =>
        Math.hypot(c.x - -17.5, c.z - 14.5);
      return d(col) < d(best) ? col : best;
    });
    const ladderTop = Math.max(...ladder.ys);

    // Stand on the bay floor just north (+z) of the ladder plane. The rungs
    // face +z there (the -z side drops into a deep shaft), and with the debug
    // runtime's default camera (facing -x) a LEFT strafe moves the player -z,
    // INTO the ladder face (mapping verified empirically).
    await game.player.teleport({
      x: ladder.x,
      y: -4.5,
      z: ladder.z + 1.5,
    });
    await game.step({ frames: 30 }); // settle onto the floor
    const before = await game.player.position();
    assert.ok(
      before.y < ladderTop,
      `expected to start below the ladder top (${ladderTop}), got y=${before.y}`,
    );

    // Press into the ladder and hold.
    await game.input.set("right_hand.thumbstick", [-1, 0]);
    await game.step({ frames: 240 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const after = await game.player.position();
    const ascent = after.y - before.y;
    assert.ok(
      ascent > 2,
      `pressing into the ladder should climb it (started y=${before.y.toFixed(2)}, ` +
        `ended y=${after.y.toFixed(2)}, ascent=${ascent.toFixed(2)}; ` +
        "without climbing the ladder collider just blocks the player)",
    );
  },
);
