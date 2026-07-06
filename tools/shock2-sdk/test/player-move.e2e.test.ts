import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Position, Vec3 } from "../src/index.js";

// End-to-end test for the bounded, shape-cast-validated player move
// (POST /v1/player/move, game.player.moveTo). Requires game assets in Data/ and
// compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: a raw teleport (the old /v1/player/teleport behaviour) warps
// the player unconditionally - straight through a wall and out of bounds. This
// test drives the player into a wall and asserts the move is CLAMPED and
// BLOCKED short of the geometry (distance_moved < requested, player stops before
// the wall). Against a raw teleport these assertions fail: it would report the
// full requested distance and land the player inside/behind the wall.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const toVec = (p: Position): Vec3 => [p.x, p.y, p.z];

test(
  "validated player move: clamps + blocks at walls, advances in open space",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8107),
    });
    await game.step({ frames: 2 });

    const spawn = await game.player.position();

    // Probe the four horizontal axes with a world raycast to find (a) a nearby
    // wall to move into and (b) an open direction with room for a full hop.
    const axes: Array<{ name: string; dir: Vec3 }> = [
      { name: "+x", dir: [1, 0, 0] },
      { name: "-x", dir: [-1, 0, 0] },
      { name: "+z", dir: [0, 0, 1] },
      { name: "-z", dir: [0, 0, -1] },
    ];
    const PROBE = 12;
    const probes = await Promise.all(
      axes.map(async (a) => {
        const end: Vec3 = [
          spawn.x + a.dir[0] * PROBE,
          spawn.y + a.dir[1] * PROBE,
          spawn.z + a.dir[2] * PROBE,
        ];
        const hit = await game.raycast({
          start: toVec(spawn),
          end,
          collision_groups: ["world"],
        });
        // distance-to-wall along the axis (Infinity when the probe finds nothing).
        const wall =
          hit.hit && hit.hit_point
            ? Math.hypot(
                hit.hit_point[0] - spawn.x,
                hit.hit_point[1] - spawn.y,
                hit.hit_point[2] - spawn.z,
              )
            : Infinity;
        return { ...a, wall };
      }),
    );

    // Nearest wall = the direction we move INTO; farthest = OPEN direction.
    const wallAxis = probes.reduce((m, p) => (p.wall < m.wall ? p : m));
    const openAxis = probes.reduce((m, p) => (p.wall > m.wall ? p : m));

    assert.ok(
      wallAxis.wall < 2.5,
      `expected a wall within 2.5u at medsci1 spawn, nearest was ${wallAxis.name} @ ${wallAxis.wall}`,
    );
    assert.ok(
      openAxis.wall > 6,
      `expected an open axis with >6u clearance, best was ${openAxis.name} @ ${openAxis.wall}`,
    );

    // --- Wall case: move toward the wall with a far (clamped) target. ---
    // A raw teleport would land at the full 5u and punch through the wall.
    const wallTarget: Position = {
      x: spawn.x + wallAxis.dir[0] * 10,
      y: spawn.y + wallAxis.dir[1] * 10,
      z: spawn.z + wallAxis.dir[2] * 10,
    };
    const blockedMove = await game.player.moveTo(wallTarget);

    assert.equal(blockedMove.blocked, true, "move into a wall should be blocked");
    assert.equal(
      blockedMove.requested_distance,
      5,
      "a 10u target should clamp the requested distance to 5",
    );
    assert.ok(
      blockedMove.distance_moved < blockedMove.requested_distance,
      `blocked move should advance less than requested, got ${blockedMove.distance_moved} of ${blockedMove.requested_distance}`,
    );
    // The player must stop SHORT of the wall - its centre never reaches the wall
    // surface (a raw teleport would sail past it).
    assert.ok(
      blockedMove.distance_moved < wallAxis.wall,
      `player should stop before the wall at ${wallAxis.wall}u, but advanced ${blockedMove.distance_moved}u`,
    );

    // --- Open-space cases: clamp to target distance, and to the 5u cap. ---
    // Near target (3u < 5u cap): advances ~3u, not blocked.
    await game.player.teleport(spawn);
    await game.step({ frames: 1 });
    const nearTarget: Position = {
      x: spawn.x + openAxis.dir[0] * 3,
      y: spawn.y + openAxis.dir[1] * 3,
      z: spawn.z + openAxis.dir[2] * 3,
    };
    const nearMove = await game.player.moveTo(nearTarget);
    assert.equal(nearMove.blocked, false, "open-space move should not be blocked");
    assert.ok(
      Math.abs(nearMove.distance_moved - 3) < 0.05,
      `near open move should advance ~3u, got ${nearMove.distance_moved}`,
    );

    // Far target (10u): clamped to the 5u cap, still clear (open axis has >6u).
    await game.player.teleport(spawn);
    await game.step({ frames: 1 });
    const farTarget: Position = {
      x: spawn.x + openAxis.dir[0] * 10,
      y: spawn.y + openAxis.dir[1] * 10,
      z: spawn.z + openAxis.dir[2] * 10,
    };
    const farMove = await game.player.moveTo(farTarget);
    assert.equal(farMove.blocked, false, "clamped open-space move should not be blocked");
    assert.ok(
      Math.abs(farMove.distance_moved - 5) < 0.05,
      `far open move should advance ~5u (the cap), got ${farMove.distance_moved}`,
    );
  },
);
