// VR hand climbing (--vr): reach a hand onto a world point, climb a ladder
// hand over hand on its real rungs or rails, and top out onto a ledge.
//
// Hands reach through `<hand>_hand.world_target`, which keeps a hand on its
// point while the body moves. A gripped hand already stays put in the world
// and the game moves the body by its controller's travel, so a PULL drives
// `<hand>_hand.position` down in pawn space instead (a world-pinned hand
// would lift nothing).
import { HttpError } from "./client.js";
import type { Game } from "./game.js";
import type { ClimbHold, LadderHolds, PlayerSnapshot, Vec3 } from "./types.js";
import { add, dot, quatRotate, scale, sub } from "./vec.js";
import type { Hand } from "./vr-pose.js";

/** A rung (each hand on its own half of the bar) or a rail (the one on the hand's side). */
export type HoldStyle = "rung" | "edge";

/** How far off the face, toward the climber, a hand closes on a hold. */
const HOLD_STANDOFF = 0.05;
/** Spacing of the hold points offered along a rail. */
const RAIL_STEP = 0.1;
/** Frames to let a world_target settle before squeezing: it trails a moving body by a frame. */
const SETTLE_FRAMES = 2;
/** One ladder stroke: the gripping controller's travel, and the frames it takes. */
const PULL = 0.5;
const PULL_FRAMES = 12;
/** A cycle that lifts the body less than this is stuck. */
const MIN_GAIN = 0.05;

export const otherHand = (hand: Hand): Hand => (hand === "left" ? "right" : "left");
const fmt = (p: Vec3) => `[${p.map((c) => c.toFixed(2)).join(", ")}]`;
const lerp = (a: Vec3, b: Vec3, t: number): Vec3 => add(a, scale(sub(b, a), t));

/** Rungs where the ladder has any, else its rails. */
export const defaultHoldStyle = (ladder: LadderHolds): HoldStyle =>
  ladder.rungs.length > 0 ? "rung" : "edge";

/**
 * Where `hand` can hold `ladder`, bottom to top, for a climber at `climber`
 * whose right is `right`: the middle of the hand's half of each rung, or points
 * along the rail on the hand's side. Each stands off the face toward the
 * climber.
 */
export function ladderHoldPoints(
  ladder: LadderHolds,
  hand: Hand,
  style: HoldStyle,
  climber: Vec3,
  right: Vec3,
): Vec3[] {
  const toward = dot(ladder.normal, sub(climber, ladder.rails[0][0])) >= 0
    ? ladder.normal
    : scale(ladder.normal, -1);
  const off = (p: Vec3) => add(p, scale(toward, HOLD_STANDOFF));
  const side = hand === "right" ? 1 : -1;
  if (style === "rung") {
    return ladder.rungs.map(([a, b]) => {
      const [l, r] = dot(a, right) <= dot(b, right) ? [a, b] : [b, a];
      return off(lerp(l, r, hand === "right" ? 0.75 : 0.25));
    });
  }
  const [bottom, top] = ladder.rails.reduce((best, rail) =>
    side * dot(rail[0], right) > side * dot(best[0], right) ? rail : best,
  );
  const steps = Math.max(1, Math.floor((top[1] - bottom[1]) / RAIL_STEP));
  return Array.from({ length: steps + 1 }, (_, i) => off(lerp(bottom, top, i / steps)));
}

/** The highest of `points` (sorted bottom to top) that `accept` takes, trying top down. */
export async function highestHold(
  points: Vec3[],
  accept: (point: Vec3) => Promise<boolean>,
): Promise<Vec3 | null> {
  for (let i = points.length - 1; i >= 0; i -= 1) {
    if (await accept(points[i])) return points[i];
  }
  return null;
}

/** Target `point` with `hand`; the runtime's refusal (e.g. out of arm's reach), or null. */
async function tryReach(game: Game, hand: Hand, point: Vec3): Promise<string | null> {
  try {
    await game.input.set(`${hand}_hand.world_target`, point);
    return null;
  } catch (error) {
    if (error instanceof HttpError && error.status === 400) return error.body;
    throw error;
  }
}

/**
 * Hold `hand`'s grip point on world `point` while the body moves. Does not
 * step. Throws with the runtime's reason when the arm cannot reach it.
 */
export async function vrReach(game: Game, hand: Hand, point: Vec3): Promise<void> {
  const refused = await tryReach(game, hand, point);
  if (refused !== null) {
    throw new Error(`vrReach: the ${hand} hand cannot reach ${fmt(point)}: ${refused}`);
  }
}

/**
 * Reach `hand`, open, onto `point`, let it settle, and close it. Returns the
 * hold it took; throws when it took none.
 */
export async function vrGrab(game: Game, hand: Hand, point: Vec3): Promise<ClimbHold> {
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await vrReach(game, hand, point);
  await game.step({ frames: SETTLE_FRAMES });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 1 });
  const hold = (await game.info()).player.climb.grips.find((grip) => grip.hand === hand);
  if (!hold) {
    const probe = (await game.physics.grip(point)).grip;
    throw new Error(
      `vrGrab: the ${hand} hand closed on nothing at ${fmt(point)} (grip probe there: ${probe?.kind ?? "none"})`,
    );
  }
  return hold;
}

/**
 * Move `hand`'s controller by `delta` in pawn space over `frames` frames: a
 * gripping hand pulled DOWN lifts the body. Takes the hand off its
 * world_target. Stops early once `until` holds. Returns the pawn position
 * after each frame.
 */
export async function vrPull(
  game: Game,
  hand: Hand,
  delta: Vec3,
  frames: number,
  until?: (player: PlayerSnapshot) => boolean,
): Promise<Vec3[]> {
  const start = (await game.input.state())[`${hand}_hand`].position;
  const path: Vec3[] = [];
  for (let frame = 1; frame <= frames; frame += 1) {
    await game.input.set(`${hand}_hand.position`, add(start, scale(delta, frame / frames)));
    await game.step({ frames: 1 });
    const { player } = await game.info();
    path.push(player.position);
    if (until?.(player)) break;
  }
  return path;
}

/** The ladder entities whose faces a column through `near` crosses from `fromY` to `toY`. */
async function laddersAlong(game: Game, near: Vec3, fromY: number, toY: number): Promise<LadderHolds[]> {
  const ids = new Set<number>();
  for (let y = fromY; y <= toY; y += 0.2) {
    const grip = (await game.physics.grip([near[0], y, near[2]])).grip;
    if (grip?.kind === "ladder" && grip.entity_id !== null) ids.add(grip.entity_id);
  }
  return Promise.all([...ids].map((id) => game.physics.ladder(id)));
}

export interface VrClimbResult {
  /** The hand holding the ladder at the end. The other is open and off its world_target. */
  anchor: Hand;
  /** The body's height after every pull and handoff frame. */
  heights: number[];
}

/**
 * Climb the ladder at `near` hand over hand until the body centre reaches
 * `untilY`. Holds come from the ladder's model (every ladder entity stacked on
 * the column through `near`): `style` "rung" or "edge", default rung where a
 * ladder has rungs. `hand` grabs first, at the highest hold in reach; each
 * stroke pulls it down while the other hand reaches for the highest hold in
 * reach, which closes before the first lets go. Throws when no hold is in
 * reach or a stroke lifts nothing.
 */
export async function vrClimbLadder(
  game: Game,
  { near, untilY, hand = "right", style }: { near: Vec3; untilY: number; hand?: Hand; style?: HoldStyle },
): Promise<VrClimbResult> {
  const { player } = await game.info();
  const right = quatRotate(player.rotation, [1, 0, 0]);
  const ladders = await laddersAlong(game, near, player.position[1] - 2, untilY + 3);
  if (ladders.length === 0) throw new Error(`vrClimbLadder: no ladder on the column through ${fmt(near)}`);
  const points = (h: Hand) =>
    ladders
      .flatMap((ladder) =>
        ladderHoldPoints(ladder, h, style ?? defaultHoldStyle(ladder), player.position, right),
      )
      .sort((a, b) => a[1] - b[1]);
  const holds = { left: points("left"), right: points("right") };
  const reachHighest = async (h: Hand) => {
    let refused = "no holds";
    const hold = await highestHold(holds[h], async (p) => {
      const reason = await tryReach(game, h, p);
      if (reason !== null) refused = reason;
      return reason === null;
    });
    if (!hold) {
      const y = (await game.info()).player.position[1];
      throw new Error(
        `vrClimbLadder: no ladder hold in the ${h} hand's reach (body at y=${y.toFixed(2)}; last refusal: ${refused})`,
      );
    }
    return hold;
  };

  let anchor = hand;
  await vrGrab(game, anchor, await reachHighest(anchor));
  const heights = [(await game.info()).player.position[1]];
  const reached = (p: PlayerSnapshot) => p.position[1] >= untilY;
  while (heights[heights.length - 1] < untilY) {
    const free = otherHand(anchor);
    const before = heights[heights.length - 1];
    // The free hand heads for a hold during the stroke, and re-picks the
    // highest one in reach once the body has risen.
    await game.input.set(`${free}_hand.squeeze`, 0);
    await reachHighest(free);
    heights.push(...(await vrPull(game, anchor, [0, -PULL, 0], PULL_FRAMES, reached)).map((p) => p[1]));
    if (heights[heights.length - 1] >= untilY) break;
    await vrGrab(game, free, await reachHighest(free));
    await game.input.set(`${anchor}_hand.squeeze`, 0);
    await game.step({ frames: 1 });
    heights.push((await game.info()).player.position[1]);
    if (heights[heights.length - 1] - before < MIN_GAIN) {
      throw new Error(`vrClimbLadder: a stroke lifted nothing (stuck at y=${before.toFixed(2)})`);
    }
    anchor = free;
  }
  await game.input.set(`${otherHand(anchor)}_hand.world_target`, null);
  return { anchor, heights };
}

/**
 * Top out onto a ledge: close `hand` on `hold` - the lip or anywhere on the
 * deck, since any walkable top above step height is a ledge hold - then
 * {@link vrVault}. Throws when `hold` is no ledge.
 */
export async function vrTopOut(
  game: Game,
  hand: Hand,
  hold: Vec3,
): Promise<{ heights: number[]; landed: Vec3 }> {
  const held = await vrGrab(game, hand, hold);
  if (held.kind !== "ledge") {
    throw new Error(`vrTopOut: ${fmt(hold)} is a ${held.kind} hold, not a ledge`);
  }
  return vrVault(game, hand);
}

/**
 * Pull `hand`, already on a ledge, down until the scripted vault starts; then
 * open both hands and let it land. Throws when the pull never vaults or the
 * vault never lands.
 */
export async function vrVault(game: Game, hand: Hand): Promise<{ heights: number[]; landed: Vec3 }> {
  const heights = (await vrPull(game, hand, [0, -1.2, 0], 40, (p) => p.climb.vaulting)).map((p) => p[1]);
  const { player } = await game.info();
  if (!player.climb.vaulting) {
    throw new Error(`vrVault: pulling the ${hand} hand never vaulted (body at y=${player.position[1].toFixed(2)})`);
  }
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("right_hand.squeeze", 0);
  // The scripted vault to its landing, then a second to settle.
  let settled = 0;
  for (let frame = 0; frame < 300 && settled < 60; frame += 1) {
    await game.step({ frames: 1 });
    const { player } = await game.info();
    heights.push(player.position[1]);
    if (!player.climb.vaulting) settled += 1;
  }
  if (settled < 60) throw new Error("vrVault: the vault did not land within 300 frames");
  return { heights, landed: (await game.info()).player.position };
}
