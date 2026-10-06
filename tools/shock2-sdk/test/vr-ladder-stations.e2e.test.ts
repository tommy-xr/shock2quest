import assert from "node:assert/strict";
import { test } from "node:test";

import {
  GameServer,
  HttpError,
  ladderHoldPoints,
  otherHand,
  vrClimbLadder,
  vrGrab,
  vrPull,
  vrTopOut,
  viewRight,
} from "../src/index.js";
import type { Hand, HoldStyle, Vec3 } from "../src/index.js";
import { shootTrail } from "./helpers/trail.js";
import { vrHandLocalDelta } from "./helpers/vr-climb.js";

// VR survey of debug_ladder's stations (shock2vr/src/scenes/debug_ladder.rs),
// driven with the SDK's hand-climbing helpers: climb hand over hand on the
// ladder model's holds, then top out, descend or exit the way a player in the
// headset would. Lip top-outs at the ledge, rung-stack and mantle stations and
// the plain-wall negative case are in vr-vault.e2e.test.ts.
//
// Helper-only step: the synthetic shoulder cannot lean, so a side floor 1.0
// from the ladder's centre is just out of an arm's reach. Side exits first
// shuffle the body along the ladder with the anchor hand (a player leans).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Every station's ladder wall is at x = -7; `d` is the distance in front of
 * it (toward the climber), `w` the offset across the lane (z - lane). */
const at = (lane: number, d: number, y: number, w = 0): Vec3 => [-7 + d, y, lane + w];
/** Capsule centre above the floor of a standing player. */
const STANDING = 1.244;
/** Capsule centre to crown, crouched and standing. */
const CROUCHED_HALF = 0.56;
const STANDING_HALF = 1.2;

/** Station point at a ladder's foot, clear of the wall for the standing body
 * (walking up to the arch ladder stops at d 0.67): a climb started closer
 * hangs with the body inside the block, and its top-out cannot be planned. */
const LADDER_FOOT: [number, number, number] = [0.7, 1.5, 0];

const CAPPED = 36;
const SETBACK = 48;
const RECESS = 60;
const TRENCH = 72;
const MIDMOUNT = 84;
const JUMP_GRAB = 96;
const LEDGE = 0;
const ARCH = 8;
const STACK = -8;
const SHORT = 16;

/** Launch in VR with the player trail recording (the pictures and the crown
 * checks read it), then stand on `lane`'s go pad or at station point `start`. */
async function launchAt(lane: number, start?: [number, number, number]): Promise<GameServer> {
  const game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
  try {
    await game.step({ frames: 5 });
    await game.devParams.set("player_trail_seconds", 120);
    await game.devParams.set("player_trail", 1);
    const [x, y, z] = start ? at(lane, ...start) : [3, 1.3, lane + 3];
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 30 });
    const arrived = (await game.info()).player.position;
    assert.ok(
      Math.abs(arrived[2] - lane) < 4 && arrived[0] < -3,
      `arrived at the ${lane} station: ${arrived.map((c) => c.toFixed(2))}`,
    );
    return game;
  } catch (error) {
    // The caller's `await using` only owns it once returned.
    await game[Symbol.asyncDispose]();
    throw error;
  }
}

const player = async (game: GameServer) => (await game.info()).player;

/** Whether `hand` can reach world `point` (a world_target the runtime accepts). */
async function reachable(game: GameServer, hand: Hand, point: Vec3): Promise<boolean> {
  try {
    await game.input.set(`${hand}_hand.world_target`, point);
    return true;
  } catch (error) {
    if (error instanceof HttpError && error.status === 400) return false;
    throw error;
  } finally {
    await game.input.set(`${hand}_hand.world_target`, null);
  }
}

async function firstReachable(game: GameServer, hand: Hand, points: Vec3[]): Promise<Vec3> {
  for (const point of points) if (await reachable(game, hand, point)) return point;
  throw new Error(`the ${hand} hand reaches none of ${JSON.stringify(points)}`);
}

/** Make `hand` the free one: the other takes the ladder at its highest hold in
 * reach, then `hand` lets go. Returns the anchor. */
async function freeHand(game: GameServer, hand: Hand, near: Vec3, style?: HoldStyle): Promise<Hand> {
  const { position, climb } = await player(game);
  assert.ok(climb.anchor_hand, "a hand holds the ladder");
  if (climb.anchor_hand !== hand) return climb.anchor_hand;
  // untilY below the body: grab only, no stroke.
  await vrClimbLadder(game, { near, untilY: position[1] - 0.2, hand: otherHand(hand), style });
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.input.set(`${hand}_hand.world_target`, null);
  await game.step({ frames: 2 });
  return otherHand(hand);
}

/** Slide the body `dw` along the lane on the anchor hand: its controller
 * moves the opposite way. */
async function shuffle(game: GameServer, anchor: Hand, dw: number) {
  await vrPull(game, anchor, await vrHandLocalDelta(game, [0, 0, -dw]), 20);
}

/** The highest crown (capsule top) the trail recorded, over samples in front
 * of the ladder wall between `d` bounds. */
async function highestCrown(game: GameServer, lane: number, d: [number, number]): Promise<number> {
  const { samples } = await game.player.trail();
  const crown = samples
    .filter((s) => Math.abs(s.pos[2] - lane) < 4 && s.pos[0] + 7 >= d[0] && s.pos[0] + 7 <= d[1])
    .reduce((top, s) => Math.max(top, s.pos[1] + (s.crouched ? CROUCHED_HALF : STANDING_HALF)), -Infinity);
  assert.ok(Number.isFinite(crown), "the trail recorded the body at the ladder");
  return crown;
}

/** Hand over hand down `holds` (sorted bottom to top) from `anchor`: raise
 * the anchor (the body lowers), the free hand takes the lowest hold in reach,
 * the anchor lets go. Every stroke must keep the hold; ends near the floor. */
async function vrClimbDown(game: GameServer, anchor: Hand, holds: Record<Hand, Vec3[]>) {
  for (let stroke = 0; stroke < 14; stroke += 1) {
    await vrPull(game, anchor, [0, 0.5, 0], 12);
    const body = await player(game);
    assert.equal(body.climb.grips.length, 1, `the ${anchor} hand held through stroke ${stroke}`);
    if (body.position[1] < 1.6) return;
    const free = otherHand(anchor);
    await vrGrab(game, free, await firstReachable(game, free, holds[free]));
    await game.input.set(`${anchor}_hand.squeeze`, 0);
    await game.step({ frames: 1 });
    anchor = free;
  }
  assert.fail(`still at y=${(await player(game)).position[1].toFixed(2)} after 14 strokes`);
}

/** Open both hands and let the body settle. */
async function letGo(game: GameServer) {
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 60 });
}

type Exit = {
  name: string;
  lane: number;
  start?: [number, number, number];
  /** Station-frame (d, y) of a point on the ladder. */
  near: [number, number];
  untilY: number;
  style?: HoldStyle;
  /** Side exits: the hand toward the floor (the LEFT is on +w facing the
   * ladder) and the shuffle toward it first. Front top-outs use the free hand. */
  side?: { hand: Hand; shuffle: number };
  /** Candidate holds (d, y, w), first reachable wins. */
  holds: [number, number, number][];
  floor: number;
  /** Where the landing must be, wholly on that floor. */
  landed: (d: number, w: number) => boolean;
  where: string;
};

// Exits that work. Side landings end a capsule radius (about 0.45) past the
// floor's edge.
const EXITS: Exit[] = [
  ...(["rung", "edge"] as const).map((style): Exit => ({
    name: `capped: side exit onto the 6.8 wing (${style})`,
    lane: CAPPED, near: [0.15, 2.6], untilY: 6.1, style,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[0.7, 6.82, 1.05], [0.6, 6.82, 1.05], [0.8, 6.82, 1.05], [0.5, 6.82, 1.05]],
    floor: 6.8, landed: (_d, w) => w > 1.0 + 0.4, where: "on the wing past w 1.4",
  })),
  {
    name: "capped: mid-deck side exit",
    lane: CAPPED, near: [0.15, 2.6], untilY: 6.1,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[0.7, 6.82, 1.3], [0.6, 6.82, 1.3], [0.8, 6.82, 1.3]],
    floor: 6.8, landed: (_d, w) => w > 1.6, where: "deeper on the wing than a lip grab",
  },
  {
    name: "setback: side exit onto the y34 floor",
    lane: SETBACK, near: [0.5, 1.6], untilY: 4.3,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[1.0, 4.42, 1.25], [0.8, 4.42, 1.25], [1.2, 4.42, 1.25]],
    floor: 4.4, landed: (_d, w) => w > 1.2 + 0.4, where: "on the floor beside the pit",
  },
  {
    // 0.45 leaves w 1.5 out of reach.
    name: "setback: mid-deck side exit",
    lane: SETBACK, near: [0.5, 1.6], untilY: 4.3,
    side: { hand: "left", shuffle: 0.75 },
    holds: [[1.0, 4.42, 1.5], [0.8, 4.42, 1.5], [1.2, 4.42, 1.5]],
    floor: 4.4, landed: (_d, w) => w > 1.8, where: "deeper beside the pit",
  },
  {
    name: "deck hole: side exit onto the deck",
    lane: RECESS, near: [0.5, 1.6], untilY: 3.2,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[0.8, 4.05, 1.3], [1.0, 4.05, 1.3], [0.6, 4.05, 1.3]],
    floor: 4.0, landed: (_d, w) => w > 1.19 + 0.4, where: "on the deck beside the hole",
  },
  {
    name: "deck hole: mid-deck side exit",
    lane: RECESS, near: [0.5, 1.6], untilY: 3.2,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[1.2, 4.05, 1.5], [1.0, 4.05, 1.6], [0.8, 4.05, 1.6]],
    floor: 4.0, landed: (_d, w) => w > 1.8, where: "deeper beside the hole",
  },
  {
    name: "trench: side exit onto the 3.2 shelf",
    lane: TRENCH, near: [0.1, 1.6], untilY: 2.6,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[0.6, 3.25, 1.1], [0.8, 3.25, 1.1], [0.6, 3.25, 1.3]],
    floor: 3.2, landed: (_d, w) => w > 0.63 + 0.4 && w < 2.23 - 0.4, where: "on the shelf (w 0.63..2.23)",
  },
  {
    name: "mid-mount: up from the lower floor, side exit onto the office floor",
    lane: MIDMOUNT, start: [0.9, 1.3, 0], near: [0.2, 1.6], untilY: 3.6,
    side: { hand: "left", shuffle: 0.45 },
    holds: [[0.8, 4.45, 1.1], [1.0, 4.45, 1.1], [0.6, 4.45, 1.1]],
    floor: 4.4, landed: (_d, w) => w > 0.98 + 0.4, where: "on the office floor beside the hole",
  },
  ...([[LEDGE, "ledge", 5.1, 6.0], [STACK, "rung stack", 8.1, 9.0]] as const).map(
    ([lane, station, untilY, top]): Exit => ({
      name: `${station}: edge-style climb, lip top-out`,
      lane, start: LADDER_FOOT, near: [0.2, 2.6], untilY, style: "edge",
      holds: [[-0.05, top + 0.05, 0]],
      floor: top, landed: (d) => d < -0.3, where: "on top of the block",
    }),
  ),
];

for (const exit of EXITS) {
  test(`debug_ladder (VR) ${exit.name}`, { skip: !e2eEnabled, timeout: 600_000 }, async () => {
    await using game = await launchAt(exit.lane, exit.start);
    const near = at(exit.lane, exit.near[0], exit.near[1]);
    const { anchor: climbed } = await vrClimbLadder(game, { near, untilY: exit.untilY, style: exit.style });
    let hand = otherHand(climbed);
    if (exit.side) {
      const anchor = await freeHand(game, exit.side.hand, near, exit.style);
      await shuffle(game, anchor, exit.side.shuffle);
      hand = exit.side.hand;
    }
    const hold = await firstReachable(game, hand, exit.holds.map(([d, y, w]) => at(exit.lane, d, y, w)));
    const { landed } = await vrTopOut(game, hand, hold);
    await shootTrail(game, `vr-${exit.name.replace(/\W+/g, "-").replace(/-$/, "")}`, { oblique: true });

    const [d, w] = [landed[0] + 7, landed[2] - exit.lane];
    assert.ok(
      Math.abs(landed[1] - (exit.floor + STANDING)) < 0.1,
      `stands on the ${exit.floor} floor: landed y=${landed[1].toFixed(3)}`,
    );
    assert.ok(exit.landed(d, w), `lands ${exit.where}: d=${d.toFixed(2)} w=${w.toFixed(2)}`);
    assert.equal((await player(game)).climb.grips.length, 0, "the top-out let go");
  });
}

// Hauling on the top hold with the head against the ceiling never pushes the
// crown through it (the flat fixes for these stations carry over: VR plans a
// crouched sweep that cannot pass a ceiling). Over-hauling (the controller
// > 0.6 past where the body can follow) breaks the grip, by design.
for (const [name, lane, near, untilY, ceiling] of [
  ["capped", CAPPED, [0.15, 2.6], 8.1, 10.0],
  ["setback", SETBACK, [0.5, 1.6], 6.0, 7.6],
  ["trench", TRENCH, [0.1, 1.6], 4.8, 6.4],
] as const) {
  test(
    `debug_ladder (VR) ${name}: hauling at the top stops the head under the ${ceiling} ceiling`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await launchAt(lane);
      const { anchor } = await vrClimbLadder(game, { near: at(lane, near[0], near[1]), untilY });
      await vrPull(game, anchor, [0, -1.0, 0], 30);
      await shootTrail(game, `vr-${name}-haul`, { oblique: true });
      const crown = await highestCrown(game, lane, [0, 1.5]);
      assert.ok(crown <= ceiling + 0.01, `crown ${crown.toFixed(3)} stays under ${ceiling}`);
      assert.ok(crown > ceiling - 0.3, `the haul reached the ceiling (crown ${crown.toFixed(3)})`);
    },
  );
}

test(
  "debug_ladder (VR) mid-mount: walk off the office floor, catch the rung stack, climb down",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchAt(MIDMOUNT);
    const rungs = Array.from({ length: 11 }, (_, i) => at(MIDMOUNT, 0.25, 0.8 * i, -0.1));
    // A falling hand trails its world point by a frame of the fall, more than
    // a rung's grip reach absorbs, so the catch takes a rail.
    const stack = await game.physics.ladder((await game.physics.grip(rungs[5])).grip!.entity_id!);
    const rails = stack.rails.flatMap(([bottom]) => rungs.map(([x, y]): Vec3 => [x, y, bottom[2]]));
    const start = await player(game);
    await game.input.lookAtWorldPoint([-17, start.position[1] + 0.5, MIDMOUNT]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    // The rungs are 1.6 from the hole's edge: only a falling body comes
    // within reach (a player would lean), so catch the first rung in reach.
    let caught: Vec3 | null = null;
    for (let frame = 0; frame < 120 && !caught; frame += 1) {
      await game.step({ frames: 1 });
      for (const rail of [...rails].sort((a, b) => b[1] - a[1])) {
        if (await reachable(game, "right", rail)) {
          caught = rail;
          break;
        }
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(caught, "a rail came within reach");
    assert.equal((await vrGrab(game, "right", caught)).kind, "ladder");
    assert.ok((await player(game)).position[1] > start.position[1] - 1.5, "caught high on the stack");

    // Down: raise the anchor hand (the body lowers), the free hand takes the
    // lowest rung in reach, and the anchor lets go.
    await vrClimbDown(game, "right", { left: rungs, right: rungs });
    await letGo(game);
    await shootTrail(game, "vr-midmount-down", { oblique: true });
    const end = (await player(game)).position;
    assert.ok(Math.abs(end[1] - STANDING) < 0.05, `stands on the lower floor: y=${end[1].toFixed(3)}`);
  },
);

test(
  "debug_ladder (VR) jump grab: jump from the pipe and catch the ladder under the ceiling",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // The go pad lands crouched on the pipe (14.8), 1.79 west (-w) of the
    // ladder. VR grips have no rise gate: a hand catches whenever it closes on
    // a hold, so the flat jump-grab fix has nothing to carry over.
    await using game = await launchAt(JUMP_GRAB);
    const start = (await player(game)).position;
    const ladders = new Set<number>();
    for (let y = 8; y <= 16.4; y += 0.4) {
      const grip = (await game.physics.grip(at(JUMP_GRAB, 0.19, y))).grip;
      if (grip?.kind === "ladder" && grip.entity_id !== null) ladders.add(grip.entity_id);
    }
    const right = await viewRight(game);
    const holds = (await Promise.all([...ladders].map((id) => game.physics.ladder(id))))
      .flatMap((ladder) => ladderHoldPoints(ladder, "left", "rung", start, right))
      .sort((a, b) => b[1] - a[1]);

    // Walk east to the takeoff (w -1.27), then jump toward the ladder.
    await game.input.lookAtWorldPoint([start[0], start[1] + 0.5, start[2] + 10]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let f = 0; f < 60 && (await player(game)).position[2] < JUMP_GRAB - 1.269; f += 1) {
      await game.step({ frames: 1 });
    }
    const takeoff = (await player(game)).position;
    assert.ok(takeoff[2] >= JUMP_GRAB - 1.269, `reached the takeoff: z=${takeoff[2].toFixed(3)}`);
    // The mission's jump: east-north-east, 20 degrees up.
    await game.input.lookAtWorldPoint([takeoff[0] - 4.132, takeoff[1] + 3.6, takeoff[2] + 9.107]);
    await game.input.set("jump", 1);
    await game.step({ frames: 1 });
    await game.input.set("jump", 0);
    let caught: Vec3 | null = null;
    for (let f = 0; f < 60 && !caught; f += 1) {
      await game.step({ frames: 1 });
      for (const hold of holds) {
        if (await reachable(game, "left", hold)) {
          caught = hold;
          break;
        }
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(caught, "a ladder hold came within reach during the jump");
    assert.equal((await vrGrab(game, "left", caught)).kind, "ladder");
    await game.step({ frames: 30 });
    await shootTrail(game, "vr-jump-grab", { oblique: true });

    const held = await player(game);
    assert.equal(held.climb.grips.length, 1, "still holding the ladder");
    assert.ok(held.position[1] >= 15.0, `caught high: y=${held.position[1].toFixed(3)}`);
    const crown = await highestCrown(game, JUMP_GRAB, [0, 7.7]);
    assert.ok(crown <= 16.4 + 0.01, `crown ${crown.toFixed(3)} stays under the 16.4 ceiling`);
  },
);

test(
  "debug_ladder (VR) arch: up the near ladder, across the top, down the far ladder",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchAt(ARCH, LADDER_FOOT);
    const { anchor } = await vrClimbLadder(game, { near: at(ARCH, 0.2, 2.6), untilY: 5.4 });
    const { landed } = await vrTopOut(game, otherHand(anchor), at(ARCH, -0.1, 6.45));
    assert.ok(Math.abs(landed[1] - (6.4 + STANDING)) < 0.1, `on the arch: y=${landed[1].toFixed(3)}`);

    // Walk to the far edge (the far ladder stands at d -2.1), crouch, and
    // hook a rail of the far ladder just under its cap.
    await game.input.lookAtWorldPoint([-20, landed[1] + 1, ARCH]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let f = 0; f < 120 && (await player(game)).position[0] > -8.6; f += 1) {
      await game.step({ frames: 1 });
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 10 });
    await game.input.set("crouch", 1);
    await game.step({ frames: 5 });
    const hookable: Vec3[] = [];
    for (const w of [-0.4, 0.4]) {
      for (const y of [6.45, 6.35]) for (const d of [-2.05, -2.1, -2.15]) hookable.push(at(ARCH, d, y, w));
    }
    assert.equal((await vrGrab(game, "right", await firstReachable(game, "right", hookable))).kind, "ladder");

    // Swing out past the edge, lower onto the ladder, then down hand over hand.
    await vrPull(game, "right", await vrHandLocalDelta(game, [0.8, 0, 0]), 60);
    await vrPull(game, "right", await vrHandLocalDelta(game, [0, 0.6, 0]), 60);
    await game.input.set("crouch", 0);
    const body = await player(game);
    assert.equal(body.climb.grips.length, 1, "swung onto the far ladder");
    const far = await game.physics.ladder((await game.physics.grip(at(ARCH, -2.15, 2.8))).grip!.entity_id!);
    const right = await viewRight(game);
    const holds = {
      left: ladderHoldPoints(far, "left", "rung", body.position, right),
      right: ladderHoldPoints(far, "right", "rung", body.position, right),
    };
    await vrClimbDown(game, "right", holds);
    await letGo(game);
    await shootTrail(game, "vr-arch", { oblique: true });
    const end = (await player(game)).position;
    assert.ok(Math.abs(end[1] - STANDING) < 0.05, `down on the floor: y=${end[1].toFixed(3)}`);
    assert.ok(end[0] < -9.2, `on the far side: x=${end[0].toFixed(3)}`);
  },
);

// The 4' ladder has two rungs (0.4, 1.2): arms alone lift the body one
// stroke, until the lower rung is out of reach. Either face; each hand takes
// its own half of a rung (the view's right, which a stick turn flips).
for (const face of ["near", "far"] as const) {
  test(
    `debug_ladder (VR) short ladder: climbs from its ${face} face`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await launchAt(SHORT, [face === "near" ? -0.5 : -1.5, 1.5, 0]);
      if (face === "far") {
        // Snap turning is edge-triggered: six 30-degree flicks face +X.
        for (let turn = 0; turn < 6; turn++) {
          await game.input.set("left_hand.thumbstick", [1, 0]);
          await game.step({ frames: 1 });
          await game.input.set("left_hand.thumbstick", [0, 0]);
          await game.step({ frames: 1 });
        }
        assert.ok((await viewRight(game))[2] > 0.999, "face the ladder from its far side");
      }
      const { heights } = await vrClimbLadder(game, { near: at(SHORT, -1, 0.8), untilY: STANDING + 0.4 });
      await shootTrail(game, `vr-short-${face}`, { oblique: true });
      assert.ok(Math.max(...heights) >= STANDING + 0.4, `rose: ${heights.map((h) => h.toFixed(2))}`);
      const right = await viewRight(game);
      const grips = (await game.player.trail()).events.filter((e) => e.kind === "grip" && e.hold === "ladder");
      assert.ok(grips.length > 0, "gripped the ladder");
      for (const { hand, pos } of grips) {
        const side = (pos[2] - SHORT) * right[2];
        assert.ok(hand === "right" ? side > 0 : side < 0, `the ${hand} hand took the far half at z=${pos[2]}`);
      }
    },
  );
}

test(
  "debug_ladder (VR) capped: a hand cannot grip the roof through the lintel",
  {
    skip: !e2eEnabled,
    timeout: 600_000,
    todo: "V1: the ledge probe has no shoulder-to-hand occlusion test, so the hand grips the 9.6 roof behind the ladder wall",
  },
  async () => {
    // From the top of the capped ladder the free hand reaches through the
    // 0.2 lintel (d 0..0.2, 9.2..10.0) onto the roof behind the ladder wall.
    // (The same climb is asserted, not todo, by the capped haul test.)
    await using game = await launchAt(CAPPED);
    const { anchor } = await vrClimbLadder(game, { near: at(CAPPED, 0.15, 2.6), untilY: 8.1 });
    await vrPull(game, anchor, [0, -0.5, 0], 15);
    const free = otherHand(anchor);
    const roofs = [at(CAPPED, -0.1, 9.62), at(CAPPED, -0.05, 9.62, -0.2), at(CAPPED, -0.05, 9.62, 0.2)];
    let roof: Vec3 | null = null;
    for (const point of roofs) if (!roof && (await reachable(game, free, point))) roof = point;
    // A fix may refuse the reach itself: then there is nothing to grip.
    if (!roof) return;
    await game.input.set(`${free}_hand.squeeze`, 0);
    await game.input.set(`${free}_hand.world_target`, roof);
    await game.step({ frames: 2 });
    await game.input.set(`${free}_hand.squeeze`, 1);
    await game.step({ frames: 1 });
    await shootTrail(game, "vr-capped-roof-through-lintel", { oblique: true });
    const held = (await player(game)).climb.grips.find((g) => g.hand === free);
    assert.notEqual(held?.kind, "ledge", `the ${free} hand closed on ${JSON.stringify(held)} behind the wall`);
  },
);
