import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

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

// Regions the body centre can never legitimately reach (duplicated from the
// scene's constants; keep in sync with debug_ladder.rs).
const CAPPED: Station = {
  lane: 36,
  // Above the 10.0 ceiling, and above the 9.6 roof behind the ladder wall.
  outside: [
    { d: [0, 7.8], y: [10.0, FAR] },
    { d: [-4, 0], y: [9.6, FAR] },
  ],
};
const SETBACK: Station = {
  lane: 48,
  // Above the deck's 11.6 ceiling, and above the shaft's (mission 45.2).
  outside: [
    { d: [0, 4.3], y: [11.6, FAR] },
    { d: [-4, 0], y: [15.6, FAR] },
  ],
};
const RECESS: Station = {
  lane: 60,
  outside: [
    { d: [0, 6], y: [7.2, FAR] },
    { d: [-4, 0], y: [11.2, FAR] },
  ],
};
const THROUGH: Station = {
  lane: 72,
  outside: [
    { d: [0, 5.6], y: [9.6, FAR] },
    { d: [-2.4, 0], y: [10.0, FAR] },
    { d: [-FAR, -2.4], y: [-FAR, FAR] },
  ],
};
const MIDMOUNT: Station = {
  lane: 84,
  // Above the 8.4 ceiling; behind the ladder wall is solid in hydro2.
  outside: [
    { d: [0, 6], y: [8.4, FAR] },
    { d: [-FAR, 0], y: [-FAR, FAR] },
  ],
};
const JUMP_GRAB: Station = {
  lane: 96,
  outside: [
    { d: [0, 7.7], y: [16.4, FAR] },
    { d: [-FAR, 0], y: [-FAR, FAR] },
  ],
};

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
    for (const r of this.station.outside) {
      const [w0, w1] = r.w ?? [-4, 4];
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

  /// Height of the walkable surface under the player, if one is within 3.
  async supportY(p: Pos): Promise<number | null> {
    const hit = await this.game.raycast({
      start: [p.x, p.y, p.z],
      end: [p.x, p.y - 3, p.z],
      collision_groups: ["world", "entity"],
    });
    return hit.hit && hit.hit_normal && hit.hit_normal[1] > 0.7 ? hit.hit_point![1] : null;
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

/// Push forward until the height stops rising for `still` frames.
async function climbToStall(run: Run, maxFrames: number, still = 30): Promise<Pos> {
  let best = -Infinity;
  let since = 0;
  return run.frames(maxFrames, (p) => {
    if (p.y > best + 0.01) {
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
    const run = new Run(game, CAPPED);
    await place(game, CAPPED, 0.729, 1.244);
    const start = await run.pos();
    const up = Math.tan(Math.PI / 3) * 10;
    await game.input.lookAtWorldPoint([start.x - 10, start.y + up, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    const cap = await climbToStall(run, 300, 3);

    // Turn back toward the corridor (a little off-axis, as in the mission)
    // and jump off with forward held.
    await game.input.lookAtWorldPoint([cap.x + 9.4, cap.y + up, cap.z + 3.42]);
    await game.input.set("jump", 1);
    await run.frames(1);
    await game.input.set("jump", 0);
    await run.frames(60);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

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
    const run = new Run(game, SETBACK);
    await place(game, SETBACK, 1.2, 0.6);
    await game.input.set("crouch", 1);
    await run.frames(10);
    await game.input.lookAtWorldPoint(at(SETBACK, 1.2 - 10.7, 11.1));
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(720, async (p) => p.y > 8.4 && (await run.supportY(p)) !== null);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

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
    const run = new Run(game, RECESS);
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    await place(game, RECESS, 0.89, 0.7);
    await game.input.lookAtWorldPoint(at(RECESS, 0.49 - 10, 11.08));
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(900, async (p) => p.y > 4.0 && (await run.supportY(p)) !== null);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(120);

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
    const run = new Run(game, THROUGH);
    await place(game, THROUGH, 0.76, 1.244);
    const start = await run.pos();
    await game.input.lookAtWorldPoint([start.x - 10, start.y + 6, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(480, async (p) => p.x - STATION_FACE_X < -0.4 && p.y > 3.2);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await run.frames(180);

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
    const run = new Run(game, MIDMOUNT);
    await place(game, MIDMOUNT, 2.0, 5.644);
    const start = await run.pos();
    // The mission's aim: 11.8 ahead and 7.644 below the start, looking down.
    await game.input.lookAtWorldPoint([start.x - 11.8, start.y - 7.644, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await run.frames(60);
    await game.input.set("right_hand.thumbstick", [0, 0]);

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
    const run = new Run(game, JUMP_GRAB);
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    // Crouched on the pipe top (14.8), 1.4 west (-w) and 0.4 in front of the
    // ladder's face line.
    await place(game, JUMP_GRAB, 1.286, 15.394, -1.789);
    const east = JUMP_GRAB.lane + (166.4 - 167.669);
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
    let prev = await run.pos();
    let grip: number | null = null;
    let frame = 0;
    await run.frames(90, (p) => {
      frame++;
      if (frame > 3 && p.y - prev.y > 0.01) grip = prev.y;
      prev = p;
      return grip !== null;
    });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    run.assertStayedInside();
    assert.ok(grip !== null && grip >= 14.1, `jump grab: gripped at ${grip} (want >= 14.1)`);
  },
);
