import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { shootTrail, startTrail } from "./helpers/trail.js";

// debug_ladder's repro stations: each rebuilds, from raycast measurements, a
// shipped-mission ladder whose climb is broken on main, and drives the same
// ordinary input (thumbstick, head aim, crouch/jump) the mission repro does.
// Each test asserts the CORRECT outcome and is marked `todo` until its fix
// lands; on main each fails the way its mission does (numbers in the station
// comments, mission heights re-based so the lowest floor is y = 0).
//
// Every station also asserts the player stayed inside it: the body centre
// never enters a region past the station's ceilings/roofs, which catches the
// "climbed out of the world" class (debug scenes have no world rep, so this
// is checked geometrically against the extents built in
// shock2vr/src/scenes/debug_ladder.rs).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/// Every station's ladder wall is at x = STATION_FACE_X; `d` is the distance
/// in front of it (toward the climber), `w` the offset across the lane.
const STATION_FACE_X = -7.0;
const FAR = 100;

type Region = { d: [number, number]; y: [number, number]; w?: [number, number] };
type Station = { lane: number; outside: Region[] };

/// Half the crouched capsule's height: a body centre this close under a
/// ceiling already has its crown through it, whatever the stance.
const CROUCH_HALF_HEIGHT = 0.56;

/// The region whose body centres put the crown through a ceiling at `y`.
function above(d: [number, number], y: number): Region {
  return { d, y: [y - CROUCH_HALF_HEIGHT, FAR] };
}

/// Solid (or outside the level) at every height.
function solid(d: [number, number], w?: [number, number]): Region {
  return { d, y: [-FAR, FAR], w };
}

// Regions the body centre can never legitimately reach, duplicated from the
// scene's constants (keep in sync with debug_ladder.rs). Slabs a correct
// top-out may cross (Dark's top-out ignores level terrain) are left out.
const CAPPED: Station = {
  lane: 36,
  // Through the 10.0 ceiling, or onto the 9.6 roof behind the ladder wall.
  outside: [above([0, 7.8], 10.0), above([-4, 0], 9.6)],
};
const SETBACK: Station = {
  lane: 48,
  // Through the deck's 11.6 ceiling, or the shaft's (mission 45.2).
  outside: [above([0, 4.3], 11.6), above([-4, 0], 15.6)],
};
const RECESS: Station = {
  lane: 60,
  // The 7.2 ceiling, its 45-degree slope behind the climber, the shaft's.
  outside: [
    above([0, 1.6], 7.2),
    ...Array.from({ length: 8 }, (_, k) =>
      above([1.6 + 0.4 * k, 2.0 + 0.4 * k], 7.2 - 0.4 * (k + 1)),
    ),
    above([-4, 0], 11.2),
  ],
};
const THROUGH: Station = {
  lane: 72,
  // The corridor's taller ceiling, the room's 10.0 ceiling, the room's far
  // wall. The 6.4 ceiling over the ladder is where the top-out crosses.
  outside: [above([1.67, 5.6], 9.6), above([-2.4, 1.67], 10.0), solid([-FAR, -2.4])],
};
const MIDMOUNT: Station = {
  lane: 84,
  // The 8.4 ceiling; behind the ladder wall is solid in hydro2.
  outside: [above([0, 6], 8.4), solid([-FAR, 0])],
};
const JUMP_GRAB: Station = {
  lane: 96,
  // The 16.4 ceiling and the shaft's walls.
  outside: [
    above([0, 7.7], 16.4),
    solid([-FAR, 0]),
    solid([-FAR, FAR], [1.93, FAR]),
    solid([-FAR, FAR], [-FAR, -3.37]),
  ],
};
/// Every station: off the side of its lane.
const OFF_LANE: Region[] = [solid([-FAR, FAR], [4.5, FAR]), solid([-FAR, FAR], [-FAR, -4.5])];

/// A world point from station-frame coordinates.
function at(station: Station, d: number, y: number, w = 0): Vec3 {
  return [STATION_FACE_X + d, y, station.lane + w];
}

type Pos = { x: number; y: number; z: number };

/// Drives the runtime a frame at a time, keeping every sampled position and
/// the first time the body centre left the station.
class Run {
  trace: Pos[] = [];
  escaped: string | null = null;

  constructor(
    private game: GameServer,
    private station: Station,
  ) {}

  async pos(): Promise<Pos> {
    return this.game.player.position();
  }

  async frames(count: number, stop?: (p: Pos) => boolean | Promise<boolean>): Promise<Pos> {
    let p = await this.pos();
    if (this.trace.length === 0) this.check(p);
    for (let i = 0; i < count; i++) {
      await this.game.step({ frames: 1 });
      p = await this.pos();
      this.check(p);
      if (stop && (await stop(p))) break;
    }
    return p;
  }

  private check(p: Pos) {
    this.trace.push(p);
    if (this.escaped) return;
    const d = p.x - STATION_FACE_X;
    const w = p.z - this.station.lane;
    for (const r of [...this.station.outside, ...OFF_LANE]) {
      const [w0, w1] = r.w ?? [-FAR, FAR];
      if (d >= r.d[0] && d <= r.d[1] && p.y >= r.y[0] && p.y <= r.y[1] && w >= w0 && w <= w1) {
        this.escaped =
          `frame ${this.trace.length}: body centre (d=${d.toFixed(3)}, y=${p.y.toFixed(3)}, ` +
          `w=${w.toFixed(3)}) left the station (region d${JSON.stringify(r.d)} y${JSON.stringify(r.y)})`;
        return;
      }
    }
  }

  assertStayedInside() {
    assert.equal(this.escaped, null, this.escaped ?? "");
  }

  /// Height of the highest walkable surface under the player's footprint
  /// (centre and four points 0.3 out, inside the crouched radius), if any
  /// is within 3.
  async supportY(p: Pos): Promise<number | null> {
    let best: number | null = null;
    for (const [dx, dz] of [[0, 0], [0.3, 0], [-0.3, 0], [0, 0.3], [0, -0.3]]) {
      const hit = await this.game.raycast({
        start: [p.x + dx, p.y, p.z + dz],
        end: [p.x + dx, p.y - 3, p.z + dz],
        collision_groups: ["world", "entity"],
      });
      if (hit.hit && hit.hit_normal && hit.hit_normal[1] > 0.7) {
        best = Math.max(best ?? -Infinity, hit.hit_point![1]);
      }
    }
    return best;
  }

  /// Ends resting on the floor at `floorY` (standing or crouched).
  async assertSupportedOn(floorY: number, label: string): Promise<Pos> {
    const p = await this.pos();
    const support = await this.supportY(p);
    assert.ok(
      support !== null && Math.abs(support - floorY) < 0.05 && p.y - floorY < 1.35,
      `${label}: expected to rest on the ${floorY} floor, ended at ` +
        `(d=${(p.x - STATION_FACE_X).toFixed(3)}, y=${p.y.toFixed(3)}) over support ${support}`,
    );
    return p;
  }
}

async function launch(): Promise<GameServer> {
  const game = await GameServer.launch({ mission: "debug_ladder" });
  await game.step({ frames: 5 });
  return game;
}

async function place(game: GameServer, station: Station, d: number, y: number, w = 0) {
  const [x, , z] = at(station, d, y, w);
  await teleportVerified(game, { x, y, z });
  await game.step({ frames: 25 });
}

/// Push forward until, above `minY`, the height stops rising for `still`
/// frames (so a hitch low on the ladder does not count as the cap).
async function climbToStall(run: Run, maxFrames: number, minY: number, still = 20): Promise<Pos> {
  let best = -Infinity;
  let since = 0;
  return run.frames(maxFrames, (p) => {
    if (p.y > best + 0.01 || p.y < minY) {
      best = p.y;
      since = 0;
    } else {
      since++;
    }
    return since >= still;
  });
}

test(
  "debug_ladder ceiling-capped ladder: head stays under the ceiling, jumping off lands on the corridor",
  // rick1 Ladder 530/532. Main: the top-out fires through the 10.0 ceiling
  // (cap y 10.59, mission 42.49) and the jump lands on the 9.6 roof
  // (y 10.84, mission 42.84) - outside the level.
  { skip: !e2eEnabled, timeout: 300_000, todo: "#1770 rick1 Ladder 532" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, CAPPED);
    await place(game, CAPPED, 0.729, 1.244);
    const start = await run.pos();
    const up = Math.tan(Math.PI / 3) * 10;
    await game.input.lookAtWorldPoint([start.x - 10, start.y + up, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    const cap = await climbToStall(run, 300, start.y + 5);

    // Turn back toward the corridor (a little off-axis, as in the mission)
    // and jump off with forward held.
    await game.input.lookAtWorldPoint([cap.x + 9.4, cap.y + up, cap.z + 3.42]);
    await game.input.set("jump", 1);
    await run.frames(1);
    await game.input.set("jump", 0);
    await run.frames(60);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

    await shootTrail(game, "capped");
    run.assertStayedInside();
    const end = await run.assertSupportedOn(6.8, "capped");
    assert.ok(end.x - STATION_FACE_X > 2.2, `capped: should land in the corridor, not the pit`);
  },
);

test(
  "debug_ladder setback ladder: a crouched climber tops out onto the deck over the pit",
  // rick1 Ladder 488. Main: crouched climbers never get a top-out; stuck
  // unsupported at y 6.871, d 0.656 under the 7.6 slab (mission 36.477,
  // 0.657 from the wall).
  { skip: !e2eEnabled, timeout: 300_000, todo: "rick1 Ladder 488 crouched top-out" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, SETBACK);
    await place(game, SETBACK, 1.2, 0.6);
    await game.input.set("crouch", 1);
    await run.frames(10);
    await game.input.lookAtWorldPoint(at(SETBACK, 1.2 - 10.7, 11.1));
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(720, async (p) => p.y > 8.4 && (await run.supportY(p)) !== null);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

    await shootTrail(game, "setback");
    run.assertStayedInside();
    await run.assertSupportedOn(8.4, "setback");
  },
);

test(
  "debug_ladder deck-hole ladder: pushing into the ladder from the top reaches the deck",
  // rick1 Ladder 499, climbed crouched from the y34 room. Main: bobs 6.41-6.60
  // at d 0.93 under the 7.2 ceiling and drops back to the room floor on
  // release (mission 40.41-40.60). Looking or strafing sideways off the top
  // already works on main; pushing on into the ladder does not.
  { skip: !e2eEnabled, timeout: 300_000, todo: "rick1 Ladder 499 crouched top-out" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, RECESS);
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    await place(game, RECESS, 0.89, 0.7);
    await game.input.lookAtWorldPoint(at(RECESS, 0.49 - 10, 11.08));
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(900, async (p) => p.y > 4.0 && (await run.supportY(p)) !== null);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

    await shootTrail(game, "recess");
    run.assertStayedInside();
    await run.assertSupportedOn(4.0, "deck hole");
  },
);

test(
  "debug_ladder through-the-wall ladder: the climb ends on the floor behind the ladder wall",
  // eng1 Ladder 317. Main: bobs 4.96-5.16 at d 0.66 under the 6.4 ceiling and
  // drops back to the corridor on release (mission -14.84/-14.64). The only
  // exit is the 3.2 floor behind the 0.4 wall the ladder is mounted on.
  { skip: !e2eEnabled, timeout: 300_000, todo: "eng1 Ladder 317 exit through the wall" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, THROUGH);
    await place(game, THROUGH, 0.76, 1.244);
    const start = await run.pos();
    await game.input.lookAtWorldPoint([start.x - 10, start.y + 6, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(480, async (p) => p.x - STATION_FACE_X < -0.4 && p.y > 3.2);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(180);

    await shootTrail(game, "through");
    run.assertStayedInside();
    const end = await run.assertSupportedOn(3.2, "through");
    assert.ok(end.x - STATION_FACE_X < -0.4, "through: should end behind the ladder wall");
  },
);

test(
  "debug_ladder mid-ladder mount: walking off the ledge onto the ladder never drops more than a climb step",
  // hydro2 Ladder 551 (#802 / PR #866). Main: drops 0.098/0.396/0.299 in
  // frames 5-7 before gripping at d 1.112 (mission 0.099/0.397/0.299).
  { skip: !e2eEnabled, timeout: 300_000, todo: "#802 hydro2 pre-grip drop (PR #866)" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, MIDMOUNT);
    await place(game, MIDMOUNT, 2.0, 5.644);
    const start = await run.pos();
    // The mission's aim: 11.8 ahead and 7.644 below the start, looking down.
    await game.input.lookAtWorldPoint([start.x - 11.8, start.y - 7.644, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(120);
    await game.input.set("right_hand.thumbstick", [0, 0]);

    await shootTrail(game, "midmount");
    run.assertStayedInside();
    const drops = run.trace.slice(1).map((p, i) => run.trace[i].y - p.y);
    const worst = Math.max(...drops);
    assert.ok(
      start.y - run.trace.at(-1)!.y > 2,
      `midmount: should have climbed down the ladder (ended y=${run.trace.at(-1)!.y.toFixed(3)})`,
    );
    assert.ok(
      worst <= 0.1,
      `midmount: largest one-frame drop ${worst.toFixed(3)} (frame ${drops.indexOf(worst) + 1}) ` +
        `exceeds a climb step`,
    );
  },
);

test(
  "debug_ladder jump-to-ladder: pressing into the ladder mid-jump grabs it high",
  // rick2 Ladder 210 (#907). Main: no grip while a jump arc is active; the
  // player falls through face reach from 15.78 and grips only at 10.97
  // (mission 94.18 -> 89.68).
  { skip: !e2eEnabled, timeout: 300_000, todo: "#907 rick2 jump grab" },
  async () => {
    await using game = await launch();
    await startTrail(game);
    const run = new Run(game, JUMP_GRAB);
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    // Crouched on the pipe top (14.8), 1.79 west (-w) of the ladder and 1.1
    // in front of it: the mission's start pose (165.88, 93.79, 9.81).
    await place(game, JUMP_GRAB, 1.286, 15.394, -1.789);
    // Walk east to the mission's takeoff point, 1.27 west of the ladder.
    const east = JUMP_GRAB.lane - 1.269;
    const walk = await run.pos();
    await game.input.lookAtWorldPoint([walk.x, walk.y, walk.z + 10]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    const stage = await run.frames(12, (p) => p.z >= east);

    // Jump east-north-east, 20 degrees up; at frame 8 turn into the ladder.
    const rise = Math.tan((20 * Math.PI) / 180) * 10;
    await game.input.lookAtWorldPoint([stage.x - 4.132, stage.y + rise, stage.z + 9.107]);
    await game.input.set("jump", 1);
    await run.frames(1);
    await game.input.set("jump", 0);
    await run.frames(7);
    await game.input.lookAtWorldPoint([stage.x - 10, stage.y + rise, stage.z]);
    let grip: number | null = null;
    await run.frames(90, async (p) => {
      if ((await game.info()).player.climb.is_climbing) grip = p.y;
      return grip !== null;
    });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    await shootTrail(game, "jump-grab");
    run.assertStayedInside();
    assert.ok(grip !== null && grip >= 14.1, `jump grab: gripped at ${grip} (want >= 14.1)`);
  },
);
