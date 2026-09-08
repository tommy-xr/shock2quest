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
const horizontalDistance = (a: Position, b: Position): number =>
  Math.hypot(b.x - a.x, b.z - a.z);

test(
  "validated player move: clamps + blocks at walls, advances in open space",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
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

test(
  "validated player move: clears a walkable corner without exceeding its hop",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 2 });

    // Regression for #599. At this airlock corner, a 210-degree walk spends
    // two frames scraping along the wall before the capsule clears its end.
    // Thumbstick locomotion keeps walking and crosses it; the validated move
    // used to treat the first low-progress frame as a terminal obstruction and
    // return after only ~0.08u.
    // Stage 0.2u farther south so the restored 2.4-foot-wide player starts
    // outside the wall's rounded-capsule contact band. The same 210-degree
    // route still spends its first frames scraping the corner before clearing
    // it, which is the validated-move behavior this regression covers.
    const corner: Position = { x: 28.36, y: 2.2, z: -66.16 };
    await game.player.teleport(corner);
    await game.step({ frames: 3 });
    const walkStart = await game.player.position();
    await game.input.set("head.look", [210, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 20 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const walkEnd = await game.player.position();
    assert.ok(
      horizontalDistance(walkStart, walkEnd) > 2,
      "fixture must remain traversable with ordinary thumbstick locomotion",
    );

    await game.player.teleport(corner);
    await game.step({ frames: 3 });
    const moveStart = await game.player.position();
    const distance = 3;
    const move = await game.player.moveTo({
      x: moveStart.x - (Math.sqrt(3) / 2) * distance,
      y: moveStart.y,
      z: moveStart.z - 0.5 * distance,
    });
    const moveEnd = await game.player.position();
    assert.ok(
      horizontalDistance(moveStart, moveEnd) > 2,
      `validated move should clear the same corner, moved ${horizontalDistance(moveStart, moveEnd)}u`,
    );

    // A different heading at the same corner used to slide 3.45u while
    // reporting a 3u request. The primitive is a bounded hop: collision
    // sliding may alter its route, but must not carry the player outside the
    // requested horizontal radius.
    await game.player.teleport(corner);
    await game.step({ frames: 3 });
    const boundStart = await game.player.position();
    const bounded = await game.player.moveTo({
      x: boundStart.x - 0.5 * distance,
      y: boundStart.y,
      z: boundStart.z + (Math.sqrt(3) / 2) * distance,
    });
    const boundEnd = await game.player.position();
    assert.ok(
      horizontalDistance(boundStart, boundEnd) <= bounded.requested_distance + 0.02,
      `a ${bounded.requested_distance}u hop moved ${horizontalDistance(boundStart, boundEnd)}u`,
    );
  },
);

test(
  "validated player move: climbs the earth Basic Training low tread",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8107),
    });
    await game.step({ frames: 2 });

    // The course initially gates this tread with two authored force fields;
    // Destroy Trap mission object 300 removes them after the lesson button.
    // Reproduce that state without relying on unstable runtime entity IDs.
    const destroyTrap = (
      await game.entities.list({ filter: "Destroy Trap", limit: 20 })
    ).entities.find((entity) => entity.template_id === 300);
    assert.ok(destroyTrap, "expected Basic Training Destroy Trap 300");
    await game.entities.sendMessage(destroyTrap.id, { type: "TurnOn" });
    await game.step({ frames: 2 });
    const forceFields = await game.entities.list({ filter: "BlueForceField", limit: 10 });
    assert.equal(forceFields.entities.length, 0, "fixture fields should be destroyed");

    // Regression for #517. The now-open authored route crosses diagonally from
    // the y=20.4 floor onto a one-foot-higher tread near z=240.4. The
    // controller used to preserve most of its speed by sliding along the
    // riser, so the step probe missed the lost z component and stopped on the
    // wrong side despite the tread being comfortably walkable.
    await game.player.teleport({ x: 6.6, y: 22.2, z: 239.6 });
    await game.step({ frames: 30 });
    const start = await game.player.position();
    const move = await game.player.moveTo({ x: 9, y: start.y, z: 240.8 });
    const end = await game.player.position();

    assert.equal(
      move.blocked,
      false,
      `walkable tread reported blocked: ${JSON.stringify(move)}`,
    );
    assert.ok(
      end.y - start.y > 0.3,
      `player should climb the tread: ${JSON.stringify({ start, end })}`,
    );
    assert.ok(
      end.z > 240.7,
      `player should cross the riser: ${JSON.stringify({ start, end })}`,
    );
  },
);
