import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, vrClimbLadder, vrGrab, vrPull } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { vrHandLocalDelta } from "./helpers/vr-climb.js";

// VR climbs with its hands: a squeeze on a hold pins the hand to the point it
// grabbed and the body moves inversely underneath it. Driven on the
// debug_ladder scene's ledge station (see shock2vr/src/scenes/debug_ladder.rs):
// a 6 wu block at x = -7 with a 16' ladder on its near face (x ~= -6.83).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** At the ledge station's ladder, on the floor: close enough for arm's reach. */
const STAND: Vec3 = [-6.3, 1.5, 0];
/** A rung of the ledge ladder, within a standing player's reach. */
const LADDER_HOLD: Vec3 = [-6.8, 2.6, 0];
/** Against the ladder: the climb helpers' holds stay in reach all the way up. */
const CLIMB_STAND: Vec3 = [-6.48, 1.5, 0];
/** The plain, non-climbable wall station. */
const WALL_HOLD: Vec3 = [-6.9, 2.3, -16];
/** The ledge ladder's rungs (ricklad6: every 0.8 from 0.4). */
const RUNG_HEIGHTS = [0.4, 1.2, 2.0, 2.8, 3.6, 4.4, 5.2, 6.0];
/** Capsule-center height of a player standing on this scene's floor. */
const FLOOR_Y = 1.24;

const launchVr = () =>
  GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });

/** Far enough from the ladder for some free travel toward it, near enough
 * to reach a rung at shoulder height. */
const STRETCH_STAND: Vec3 = [-5.95, 1.5, 0];
const STRETCH_HOLD: Vec3 = [-6.8, 2.0, 0];
const INTO_STEP = 0.06;
const INTO_STEPS = 48;

/** Haul the gripping right hand toward the body, one step a frame, until the
 * grip breaks; the step it broke at, or 0 when it held throughout. */
async function pullIntoTheWall(game: GameServer): Promise<number> {
  const into = await vrHandLocalDelta(game, [INTO_STEP * INTO_STEPS, 0, 0]);
  const path = await vrPull(game, "right", into, INTO_STEPS, (p) => p.climb.grips.length === 0);
  return (await game.info()).player.climb.grips.length === 0 ? path.length : 0;
}

async function standAtTheLadder(game: GameServer, at: Vec3 = STAND) {
  await game.step({ frames: 5 });
  await teleportVerified(game, { x: at[0], y: at[1], z: at[2] });
  await game.step({ frames: 30 });
}

test(
  "debug_ladder (VR): pulling a gripped hand down lifts the body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    const before = (await game.info()).player.position;
    await vrGrab(game, "right", LADDER_HOLD);
    const after = (await vrPull(game, "right", [0, -1.0, 0], 30)).at(-1)!;

    assert.ok(
      Math.abs(after[1] - before[1] - 1.0) < 0.15,
      `expected the body to rise by the pull, ${before[1]} -> ${after[1]}`,
    );
    const climb = (await game.info()).player.climb;
    assert.equal(climb.anchor_hand, "right");
    assert.equal(climb.is_climbing, true);
    assert.equal(climb.grips.length, 1);
    assert.equal(climb.grips[0].kind, "ladder");

    // The hand stayed on the hold it grabbed - that is the whole mechanic.
    const held = climb.grips[0].point;
    assert.ok(
      Math.abs(held[1] - LADDER_HOLD[1]) < 0.1,
      `the grip should stay at the grabbed height, got ${held.join(",")}`,
    );

    // Letting go drops the player back to the floor.
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 60 });
    const landed = await game.info();
    assert.equal(landed.player.climb.grips.length, 0);
    assert.equal(landed.player.climb.anchor_hand, null);
    assert.ok(
      Math.abs(landed.player.position[1] - FLOOR_Y) < 0.2,
      `expected a fall back to the floor, got ${landed.player.position[1]}`,
    );
  },
);

test(
  "debug_ladder (VR): the second hand takes over without snapping the body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game, CLIMB_STAND);

    const rightHold: Vec3 = [-6.8, 2.3, 0];
    const leftHold: Vec3 = [-6.8, 2.7, 0];

    // Right hand on, then the left higher up: the last hand to grab drives.
    await vrGrab(game, "right", rightHold);
    await vrGrab(game, "left", leftHold);
    assert.equal((await game.info()).player.climb.anchor_hand, "left");

    // Pull with the left. The right hand is left where it is: a hand that is
    // holding on does not move relative to the player, the player moves.
    const PULL = 0.4;
    const heights: number[] = [(await game.info()).player.position[1]];
    heights.push(...(await vrPull(game, "left", [0, -PULL, 0], 20)).map((p) => p[1]));
    assert.ok(
      heights[heights.length - 1] - heights[0] > PULL - 0.1,
      `expected the left pull to lift the body, ${heights[0]} -> ${heights[heights.length - 1]}`,
    );

    // Release the anchor. The right hand is still on, so it takes over - and
    // must not yank the body back to where the right hand first grabbed.
    await game.input.set("left_hand.squeeze", 0);
    for (let frame = 0; frame < 10; frame += 1) {
      await game.step({ frames: 1 });
      heights.push((await game.info()).player.position[1]);
    }
    const handoff = (await game.info()).player.climb;
    assert.equal(handoff.anchor_hand, "right");
    assert.equal(handoff.grips.length, 1);

    for (let i = 1; i < heights.length; i += 1) {
      assert.ok(
        Math.abs(heights[i] - heights[i - 1]) < 0.1,
        `the body jumped ${heights[i - 1]} -> ${heights[i]} in one frame`,
      );
    }
  },
);

test(
  "debug_ladder (VR): a hand the body cannot follow stretches off the hold",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game, STRETCH_STAND);
    await vrGrab(game, "right", STRETCH_HOLD);
    assert.equal((await game.info()).player.climb.grips.length, 1);

    // Pull the hand back toward the body, a step at a time: pinned to the
    // hold, that hauls the BODY into the wall the ladder hangs on. Each step
    // is far short of the break distance, so nothing breaks on the reach
    // alone - the grip only comes off once the wall stops the body and the
    // separation accumulates.
    const broke = await pullIntoTheWall(game);

    // The player has ~0.6 wu of floor before the block stops them, so the
    // first several steps are free travel the body follows exactly - proof the
    // break comes from the blocked body and not from the reach itself.
    assert.ok(
      broke > 5,
      `an unobstructed reach must not break the grip, broke at step ${broke}`,
    );
    assert.ok(
      broke <= 40,
      `a blocked body must break the grip, still held after ${INTO_STEP * INTO_STEPS} wu`,
    );
    assert.equal((await game.info()).player.climb.anchor_hand, null);
  },
);

test(
  "debug_ladder (VR): squeezing on the plain wall grabs nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game, [STAND[0], STAND[1], -16]);

    const before = (await game.info()).player.position;
    await assert.rejects(vrGrab(game, "right", WALL_HOLD), /closed on nothing/);
    const after = (await vrPull(game, "right", [0, -1.0, 0], 30)).at(-1)!;

    assert.equal((await game.info()).player.climb.grips.length, 0);
    assert.ok(
      Math.abs(after[1] - before[1]) < 0.1,
      `a wall is not a hold, but the body moved ${before[1]} -> ${after[1]}`,
    );
  },
);

test(
  "debug_ladder (VR): pushing the stick into a ladder does not climb",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    const floorY = (await game.info()).player.position[1];
    await game.input.lookAtWorldPoint([
      -7,
      floorY + (await game.info()).player.camera_offset[1],
      0,
    ]);
    await game.step({ frames: 5 });

    // Every stick direction, so whichever one is "into the ladder" for this
    // pawn is covered. Hands are the climb input in VR (see vr_climb).
    for (const stick of [
      [0, 1],
      [0, -1],
      [1, 0],
      [-1, 0],
    ]) {
      await teleportVerified(game, { x: STAND[0], y: STAND[1], z: STAND[2] });
      await game.step({ frames: 20 });
      await game.input.set("right_hand.thumbstick", stick);
      await game.step({ frames: 120 });
      const y = (await game.info()).player.position[1];
      assert.ok(
        y < floorY + 0.3,
        `stick ${stick.join(",")} climbed to y=${y} in VR`,
      );
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
  },
);

/** Grab LADDER_HOLD, pull the right hand down 1 wu over `frames`, open it;
 * the body position just after. */
async function pullAndLetGo(game: GameServer, frames: number): Promise<Vec3> {
  await vrGrab(game, "right", LADDER_HOLD);
  await vrPull(game, "right", [0, -1.0, 0], frames);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
  return (await game.info()).player.position;
}

/** Every pawn height while stepping `frames` frames, one at a time. */
async function stepTrackingHeight(
  game: GameServer,
  frames: number,
): Promise<number[]> {
  const heights: number[] = [];
  for (let frame = 0; frame < frames; frame += 1) {
    await game.step({ frames: 1 });
    heights.push((await game.info()).player.position[1]);
  }
  return heights;
}

test(
  "debug_ladder (VR): letting go of a hard pull throws the body upward",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    // A wrench of a pull: 1 wu of hand travel in 6 frames.
    const after = await pullAndLetGo(game, 6);

    const heights = await stepTrackingHeight(game, 120);
    const peak = Math.max(...heights);
    assert.ok(
      peak > after[1] + 0.3,
      `the release should have thrown the body up from ${after[1]}, peaked at ${peak}`,
    );
    assert.equal((await game.info()).player.climb.grips.length, 0);
    // ... and the arc ends: whatever it landed on, it is no longer rising.
    const settled = heights.slice(-10);
    assert.ok(
      Math.max(...settled) - Math.min(...settled) < 0.1,
      `expected a landing, still moving: ${settled.join(",")}`,
    );
  },
);

test(
  "debug_ladder (VR): letting go of a pull below the deadzone throws nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    // The same 1 wu of hand travel at 0.33 wu/s - under the release
    // deadzone (CLIMB_RELEASE_MIN_SPEED), so letting go is just letting go.
    const after = await pullAndLetGo(game, 180);

    const peak = Math.max(...(await stepTrackingHeight(game, 120)));
    assert.ok(
      peak < after[1] + 0.1,
      `a gentle release should just drop the player, ${after[1]} -> peak ${peak}`,
    );
  },
);

test(
  "debug_ladder (VR): a grip torn off by a blocked body throws nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();

    // Haul the body into the wall the ladder hangs on until the grip breaks
    // (the same blocked-body break the stretch test drives), then check that
    // the recorded "pull" - which was the body failing to follow - did not
    // become a launch.
    await standAtTheLadder(game, STRETCH_STAND);
    await vrGrab(game, "right", STRETCH_HOLD);
    const brokeAt = await pullIntoTheWall(game);
    assert.ok(brokeAt > 0, "the blocked body should have broken the grip");

    const broken = (await game.info()).player.position[1];
    const peak = Math.max(...(await stepTrackingHeight(game, 90)));
    assert.ok(
      peak < broken + 0.1,
      `a broken grip must not launch, ${broken} -> peak ${peak}`,
    );
  },
);

for (const style of ["rung", "edge"] as const) {
  test(
    `debug_ladder (VR): hand over hand climbs the ladder by its ${style}s`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await launchVr();
      await standAtTheLadder(game, CLIMB_STAND);

      const start = (await game.info()).player.position[1];
      const { anchor, heights } = await vrClimbLadder(game, {
        near: LADDER_HOLD,
        untilY: start + 2.5,
        style,
      });

      const climb = (await game.info()).player.climb;
      assert.equal(climb.anchor_hand, anchor);
      assert.equal(climb.grips.length, 1);
      assert.equal(climb.grips[0].kind, "ladder");
      // Each hand closed on the ladder's own geometry: a rung, or its rail.
      const held = climb.grips[0].point;
      if (style === "rung") {
        assert.ok(
          RUNG_HEIGHTS.some((y) => Math.abs(held[1] - y) < 0.05),
          `the ${anchor} hand should hold a rung, got ${held.join(",")}`,
        );
      } else {
        assert.ok(Math.abs(Math.abs(held[2]) - 0.4) < 0.05, `the ${anchor} hand should hold a rail, got ${held.join(",")}`);
      }
      for (let i = 1; i < heights.length; i += 1) {
        assert.ok(
          Math.abs(heights[i] - heights[i - 1]) < 0.1,
          `the body jumped ${heights[i - 1]} -> ${heights[i]} in one frame`,
        );
      }
    },
  );
}


test("VR hand anticipates a reachable ladder without starting a climb", { skip: !e2eEnabled, timeout: 180_000 }, async () => {
  await using game = await launchVr();
  await standAtTheLadder(game);
  await game.input.set("right_hand.world_target", LADDER_HOLD);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({frames: 30});
  const player = (await game.info()).player;
  assert.equal(player.climb.grips.length, 0);
  assert.ok(player.hand_feedback!.anticipation[1].curls[2] > 0.5);
});
