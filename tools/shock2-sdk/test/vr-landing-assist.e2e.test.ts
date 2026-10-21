import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, otherHand, quatMultiply, quatRotate, viewRight, vrClimbLadder, vrGrab } from "../src/index.js";
import type { ClimbRelease, Hand, Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// Landing assist (#2376): while hanging from a VR hold, /v1/info
// climb.release says where letting go of every hold lands (the footprint and
// the held gloves' green/red show it). Looking at a nearby walkable landing
// from a ladder or grace hold makes it an assisted landing: a voluntary final
// release glides there, frozen at release. Driven at debug_ladder's ledge
// top-out (z = 0) and mid-ladder exit (z = -26) stations.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const LEDGE_Z = 0;
/** The ledge block's deck, behind its 16' ladder. */
const DECK_Y = 6.0;
const SIDE_Z = -26;
/** Both mid-ladder landings' tops. */
const PLATFORM_Y = 4.0;
/** Eye height above the body centre with the default tracked head. */
const EYE_ABOVE_CENTER = 1.04;

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const horizontal = (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[2] - b[2]);

async function release(game: GameServer): Promise<ClimbRelease> {
  return (await game.info()).player.climb.release;
}

/**
 * An aimer for the tracked head: `look(target)` aims it at a world point and
 * returns the release it resolves. The view basis is read once, with the head
 * level, so successive aims move the head the way a player would.
 */
async function aimer(game: GameServer): Promise<(target: Vec3) => Promise<ClimbRelease>> {
  await game.input.set("head.look", [0, 0]);
  await game.step({ frames: 1 });
  const { player } = await game.info();
  const view = quatMultiply(player.rotation, (await game.input.state()).head.rotation);
  const forward = quatRotate(view, [0, 0, -1]);
  const right = quatRotate(view, [1, 0, 0]);
  return async (target) => {
    const at = (await game.info()).player.position;
    const d: Vec3 = [target[0] - at[0], target[1] - at[1] - EYE_ABOVE_CENTER, target[2] - at[2]];
    const across = Math.hypot(dot(d, right), dot(d, forward));
    await game.input.set("head.look", [
      (Math.atan2(dot(d, right), dot(d, forward)) * 180) / Math.PI,
      (Math.atan2(-d[1], across) * 180) / Math.PI,
    ]);
    await game.step({ frames: 2 });
    return release(game);
  };
}

/** Open `hand`, then turn the head away, step until the body settles; its position. */
async function letGoAndSettle(game: GameServer, hand: Hand): Promise<{ committed: Vec3 | null; landed: Vec3 }> {
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 1 });
  const committed = (await release(game)).gliding_to;
  // The head no longer steers a committed landing.
  await game.input.set("head.look", [120, 30]);
  await game.step({ frames: 150 });
  const { player } = await game.info();
  assert.equal(player.climb.vaulting, false, "the glide finished");
  return { committed, landed: player.position };
}

async function climbAt(game: GameServer, z: number, untilY: number) {
  await game.step({ frames: 5 });
  await teleportVerified(game, { x: -6.48, y: 1.5, z });
  await game.step({ frames: 30 });
  return vrClimbLadder(game, { near: [-6.85, 2.0, z], untilY });
}

function assertArrived(landed: Vec3, shown: Vec3) {
  assert.ok(horizontal(landed, shown) < 0.05, `arrived at ${landed}, shown ${shown}`);
  assert.ok(Math.abs(landed[1] - shown[1]) < 0.05, `arrived at ${landed}, shown ${shown}`);
}

test(
  "debug_ladder (VR): looking at the deck from a ladder's top shows a safe assisted landing a release glides to",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    const { anchor } = await climbAt(game, LEDGE_Z, 6.2);

    const lookAt = await aimer(game);
    // Looking up into open air: no gaze landing, so the natural drop shows, red.
    let shown = await lookAt([-7.5, 10.0, LEDGE_Z]);
    assert.equal(shown.mode, "natural");
    assert.equal(shown.rejection, "no_hit");
    assert.equal(shown.safe, false, "a 5-unit drop is not safe");

    shown = await lookAt([-8.0, DECK_Y, LEDGE_Z]);
    assert.equal(shown.mode, "assisted", JSON.stringify(shown));
    assert.equal(shown.safe, true);
    assert.ok(Math.abs((shown.floor_y ?? 0) - DECK_Y) < 0.01);
    const landing = shown.landing!;
    // Aim jitter keeps the same landing.
    assert.deepEqual((await lookAt([-8.3, DECK_Y, LEDGE_Z + 0.3])).landing, landing);
    await lookAt([-8.0, DECK_Y, LEDGE_Z]);

    const { committed, landed } = await letGoAndSettle(game, anchor);
    assert.deepEqual(committed, landing, "the shown landing is committed");
    assertArrived(landed, landing);
  },
);

test(
  "debug_ladder (VR): mid-ladder, a looked-at side platform is the release's landing; too far falls back to the natural drop",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    const { anchor } = await climbAt(game, SIDE_Z, 5.3);

    const lookAt = await aimer(game);
    // Beyond assist range: the natural drop to the floor, and a long one.
    let shown = await lookAt([-5.0, PLATFORM_Y, SIDE_Z + 2.0]);
    assert.equal(shown.rejection, "too_far");
    assert.equal(shown.mode, "natural");
    const below = (await game.info()).player.position;
    assert.ok(horizontal(shown.landing!, below) < 0.05, "straight down");
    assert.ok((shown.expected_drop ?? 0) > 3, "a long drop");
    assert.equal(shown.safe, false);

    // The side platform, beside the ladder.
    shown = await lookAt([-6.3, PLATFORM_Y, SIDE_Z + 2.0]);
    assert.equal(shown.mode, "assisted", JSON.stringify(shown));
    assert.equal(shown.safe, true);
    const landing = shown.landing!;
    assert.ok(landing[2] > SIDE_Z + 1.4 + 0.3, `on the platform, inset: ${landing}`);

    const { committed, landed } = await letGoAndSettle(game, anchor);
    assert.deepEqual(committed, landing);
    assertArrived(landed, landing);
  },
);

test(
  "debug_ladder (VR): crouching while hanging still glides to the shown landing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    const { anchor } = await climbAt(game, SIDE_Z, 5.3);
    const lookAt = await aimer(game);
    await lookAt([-6.3, PLATFORM_Y, SIDE_Z + 2.0]);
    await game.input.set("crouch", 1);
    await game.step({ frames: 30 });
    const shown = await release(game);
    assert.equal(shown.mode, "assisted", JSON.stringify(shown));
    const { committed, landed } = await letGoAndSettle(game, anchor);
    assert.deepEqual(committed, shown.landing, "the shown landing is what a crouched release commits");
    assert.ok(horizontal(landed, shown.landing!) < 0.05, `arrived at ${landed}, shown ${shown.landing}`);
    assert.ok(landed[1] > PLATFORM_Y, `on the platform: ${landed}`);
  },
);

test(
  "debug_ladder (VR): a grace hold on the wall still offers the assisted exit, and a handoff commits nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    const { anchor } = await climbAt(game, SIDE_Z, 5.3);
    const wallHand = otherHand(anchor);
    const y = (await game.info()).player.position[1];
    const right = Math.sign((await viewRight(game))[2]);
    // Bare wall beside the ladder, on the wall hand's side.
    const wall: Vec3 = [-6.9, y + 0.3, SIDE_Z + (wallHand === "right" ? right : -right) * 0.7];
    assert.equal((await vrGrab(game, wallHand, wall)).kind, "surface");

    // Letting go of the ladder is a handoff to the wall hold: no glide.
    await game.input.set(`${anchor}_hand.squeeze`, 0);
    await game.input.set(`${anchor}_hand.world_target`, null);
    await game.step({ frames: 2 });
    let climb = (await game.info()).player.climb;
    assert.equal(climb.grips.length, 1);
    assert.equal(climb.release.gliding_to, null, "a handoff is not a release");

    const shown = await (await aimer(game))([-4.6, PLATFORM_Y, SIDE_Z]);
    assert.equal(shown.mode, "assisted", JSON.stringify(shown));
    await game.input.set(`${wallHand}_hand.world_target`, null);
    const { committed, landed } = await letGoAndSettle(game, wallHand);
    assert.deepEqual(committed, shown.landing);
    assertArrived(landed, shown.landing!);
    climb = (await game.info()).player.climb;
    assert.equal(climb.grips.length, 0);
  },
);

test(
  "debug_ladder (VR): low on a ladder the natural drop is safe",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    await climbAt(game, SIDE_Z, 1.8);
    const shown = await (await aimer(game))([-7.0, 2.5, SIDE_Z]);
    assert.equal(shown.mode, "natural");
    // Within the 1 m default `climb_safe_drop` (1 / 0.762 world units).
    assert.ok((shown.expected_drop ?? 99) < 1 / 0.762, JSON.stringify(shown));
    assert.equal(shown.safe, true);
  },
);
