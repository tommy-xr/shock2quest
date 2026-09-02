import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import {
  vrClimbPull,
  vrHandLocal,
  vrHandLocalDelta,
} from "./helpers/vr-climb.js";

// VR climbs with its hands: a squeeze on a hold pins the hand to the point it
// grabbed and the body moves inversely underneath it. Driven on the
// debug_ladder scene's ledge station (see shock2vr/src/scenes/debug_ladder.rs):
// a 6 wu block at x = -7 with a 16' ladder on its near face (x ~= -6.83).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** In front of the ledge station's ladder, on the floor. */
const STAND: Vec3 = [-5.5, 1.5, 0];
/** A rung of the ledge ladder, within a standing player's reach. */
const LADDER_HOLD: Vec3 = [-6.8, 2.6, 0];
/** The plain, non-climbable wall station. */
const WALL_HOLD: Vec3 = [-6.9, 2.6, -16];
/** Capsule-center height of a player standing on this scene's floor. */
const FLOOR_Y = 1.24;

const launchVr = () =>
  GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });

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

    const { before, after } = await vrClimbPull(game, {
      hand: "right",
      grabAt: LADDER_HOLD,
      pull: [0, -1.0, 0],
      frames: 30,
    });

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
    await standAtTheLadder(game);

    const rightHold: Vec3 = [-6.8, 2.4, 0];
    const leftHold: Vec3 = [-6.8, 3.0, 0];

    // Right hand on, then the left higher up: the last hand to grab drives.
    await game.input.set("right_hand.position", await vrHandLocal(game, rightHold));
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });
    const leftAtGrab = await vrHandLocal(game, leftHold);
    await game.input.set("left_hand.position", leftAtGrab);
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.climb.anchor_hand, "left");

    // Pull with the left. The right hand is left where it is: a hand that is
    // holding on does not move relative to the player, the player moves.
    const heights: number[] = [(await game.info()).player.position[1]];
    const PULL = 0.4;
    const FRAMES = 20;
    const down = await vrHandLocalDelta(game, [0, -PULL, 0]);
    for (let frame = 1; frame <= FRAMES; frame += 1) {
      await game.input.set("left_hand.position", [
        leftAtGrab[0] + (down[0] * frame) / FRAMES,
        leftAtGrab[1] + (down[1] * frame) / FRAMES,
        leftAtGrab[2] + (down[2] * frame) / FRAMES,
      ]);
      await game.step({ frames: 1 });
      heights.push((await game.info()).player.position[1]);
    }
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
    await standAtTheLadder(game);

    await game.input.set(
      "right_hand.position",
      await vrHandLocal(game, LADDER_HOLD),
    );
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });
    assert.equal((await game.info()).player.climb.grips.length, 1);

    // Pull the hand back toward the body, a step at a time: pinned to the
    // hold, that hauls the BODY into the wall the ladder hangs on. Each step
    // is far short of the break distance, so nothing breaks on the reach
    // alone - the grip only comes off once the wall stops the body and the
    // separation accumulates.
    const STEP = 0.12;
    const atGrab = await vrHandLocal(game, LADDER_HOLD);
    const into = await vrHandLocalDelta(game, [STEP, 0, 0]);
    let broke = 0;
    for (let frame = 1; frame <= 24; frame += 1) {
      await game.input.set("right_hand.position", [
        atGrab[0] + into[0] * frame,
        atGrab[1] + into[1] * frame,
        atGrab[2] + into[2] * frame,
      ]);
      await game.step({ frames: 1 });
      if ((await game.info()).player.climb.grips.length === 0) {
        broke = frame;
        break;
      }
    }

    // The player has ~0.8 wu of floor before the block stops them, so the
    // first several steps are free travel the body follows exactly - proof the
    // break comes from the blocked body and not from the reach itself.
    assert.ok(
      broke > 5,
      `an unobstructed reach must not break the grip, broke at step ${broke}`,
    );
    assert.ok(
      broke <= 20,
      `a blocked body must break the grip, still held after ${STEP * 24} wu`,
    );
    assert.equal((await game.info()).player.climb.anchor_hand, null);
  },
);

test(
  "debug_ladder (VR): squeezing on the plain wall grabs nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game, [-5.5, 1.5, -16]);

    const { before, after } = await vrClimbPull(game, {
      hand: "right",
      grabAt: WALL_HOLD,
      pull: [0, -1.0, 0],
      frames: 30,
    });

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
    const { after } = await vrClimbPull(game, {
      hand: "right",
      grabAt: LADDER_HOLD,
      pull: [0, -1.0, 0],
      frames: 6,
      release: true,
    });

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
  "debug_ladder (VR): letting go of a slow pull does not throw the body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    // The same 1 wu of hand travel, at a leisurely 1 wu per second.
    const { after } = await vrClimbPull(game, {
      hand: "right",
      grabAt: LADDER_HOLD,
      pull: [0, -1.0, 0],
      frames: 60,
      release: true,
    });

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
    await standAtTheLadder(game);

    // Haul the body into the wall the ladder hangs on until the grip breaks
    // (the same blocked-body break the stretch test drives), then check that
    // the recorded "pull" - which was the body failing to follow - did not
    // become a launch.
    const atGrab = await vrHandLocal(game, LADDER_HOLD);
    const into = await vrHandLocalDelta(game, [0.12, 0, 0]);
    await game.input.set("right_hand.position", atGrab);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 1 });

    let brokeAt: number | null = null;
    for (let frame = 1; frame <= 24 && brokeAt === null; frame += 1) {
      await game.input.set("right_hand.position", [
        atGrab[0] + into[0] * frame,
        atGrab[1] + into[1] * frame,
        atGrab[2] + into[2] * frame,
      ]);
      await game.step({ frames: 1 });
      if ((await game.info()).player.climb.grips.length === 0) {
        brokeAt = frame;
      }
    }
    assert.ok(brokeAt !== null, "the blocked body should have broken the grip");

    const broken = (await game.info()).player.position[1];
    const peak = Math.max(...(await stepTrackingHeight(game, 90)));
    assert.ok(
      peak < broken + 0.1,
      `a broken grip must not launch, ${broken} -> peak ${peak}`,
    );
  },
);

test(
  "debug_ladder (VR): hand over hand climbs the ladder",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAtTheLadder(game);

    // Hands are tracked in PAWN space, so one reach height serves every
    // cycle: as the body rises, the same reach lands on a higher rung.
    const reach = await vrHandLocal(game, LADDER_HOLD);
    const PULL = 0.5;
    const FRAMES = 12;
    const down = await vrHandLocalDelta(game, [0, -PULL, 0]);
    const lowered: Vec3 = [
      reach[0] + down[0],
      reach[1] + down[1],
      reach[2] + down[2],
    ];

    const heights = [(await game.info()).player.position[1]];
    let pulling: "left" | "right" = "right";
    await game.input.set("right_hand.position", reach);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.squeeze", 1);
    heights.push(...(await stepTrackingHeight(game, 1)));

    for (let half = 0; half < 6; half += 1) {
      // Haul the gripping hand down: the body comes up to meet it.
      for (let frame = 1; frame <= FRAMES; frame += 1) {
        await game.input.set(`${pulling}_hand.position`, [
          reach[0] + (down[0] * frame) / FRAMES,
          reach[1] + (down[1] * frame) / FRAMES,
          reach[2] + (down[2] * frame) / FRAMES,
        ]);
        heights.push(...(await stepTrackingHeight(game, 1)));
      }
      assert.equal(
        (await game.info()).player.climb.anchor_hand,
        pulling,
        `the ${pulling} hand should be driving on half-cycle ${half}`,
      );

      // The other hand reaches past it and takes over, then this one lets go
      // while still holding on - a handoff, not a release.
      const other = pulling === "right" ? "left" : "right";
      await game.input.set(`${other}_hand.position`, reach);
      await game.input.set(`${other}_hand.squeeze`, 0);
      heights.push(...(await stepTrackingHeight(game, 1)));
      await game.input.set(`${other}_hand.squeeze`, 1);
      heights.push(...(await stepTrackingHeight(game, 1)));
      await game.input.set(`${pulling}_hand.squeeze`, 0);
      await game.input.set(`${pulling}_hand.position`, lowered);
      heights.push(...(await stepTrackingHeight(game, 1)));

      const climb = (await game.info()).player.climb;
      assert.equal(climb.anchor_hand, other, `handoff failed on half ${half}`);
      assert.equal(climb.grips.length, 1);
      pulling = other;
    }

    const climbed = heights[heights.length - 1] - heights[0];
    assert.ok(climbed > 2.5, `hand over hand climbed only ${climbed} wu`);
    for (let i = 1; i < heights.length; i += 1) {
      assert.ok(
        Math.abs(heights[i] - heights[i - 1]) < 0.1,
        `the body jumped ${heights[i - 1]} -> ${heights[i]} in one frame`,
      );
    }
  },
);
