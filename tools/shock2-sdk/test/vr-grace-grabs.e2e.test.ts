import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, otherHand, viewRight, vrClimbLadder, vrReach } from "../src/index.js";
import type { Hand, Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// Grace grabs (#2376): while one hand holds a ladder, the free hand may take
// one hold on any static surface near it, and one more from that hold - never
// a third. The glove's in-reach glow (`climb.reach`) answers by the same rule.
// Driven at debug_ladder's mid-ladder exit station (z = -26): a 16' ladder up
// a wall too tall to top out.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SIDE_Z = -26;
/** The station wall's face; ladder rails sit at z = -26 +- 0.4. */
const WALL_X = -7.0;
const RUNG_NEAR: Vec3 = [-6.85, 2.0, SIDE_Z];
/** Body centre partway up, level with the side landing's deck. */
const CLIMB_TO = 5.3;

async function climbState(game: GameServer) {
  return (await game.info()).player.climb;
}

/** Move `hand` (open) onto a world point and report its glow. */
async function reachOpen(game: GameServer, hand: Hand, point: Vec3) {
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await vrReach(game, hand, point);
  await game.step({ frames: 3 });
  return (await climbState(game)).reach[hand === "left" ? 0 : 1];
}

/** Squeeze `hand` where it is; the hold it took, if any. */
async function squeeze(game: GameServer, hand: Hand) {
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 2 });
  return (await climbState(game)).grips.find((g) => g.hand === hand) ?? null;
}

async function open(game: GameServer, hand: Hand) {
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.input.set(`${hand}_hand.world_target`, null);
  await game.step({ frames: 2 });
}

for (const first of ["right", "left"] as const) {
  test(
    `debug_ladder (VR): two grace grabs on the wall beside a ladder, never a third (${first} climbs first)`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
      await game.step({ frames: 5 });
      await teleportVerified(game, { x: -6.48, y: 1.5, z: SIDE_Z });
      await game.step({ frames: 30 });
      const { anchor: ladderHand } = await vrClimbLadder(game, { near: RUNG_NEAR, untilY: CLIMB_TO, hand: first });
      const wallHand = otherHand(ladderHand);
      const y = (await game.info()).player.position[1];
      // Bare wall beside the ladder, clear of its rails, on `hand`'s side.
      const right = Math.sign((await viewRight(game))[2]);
      const wall = (hand: Hand, dy: number): Vec3 => [
        WALL_X + 0.1,
        y + dy,
        SIDE_Z + (hand === "right" ? right : -right) * 0.7,
      ];

      // Free air near the ladder: no glow, nothing to take.
      assert.equal(await reachOpen(game, wallHand, [WALL_X + 0.5, y + 0.3, wall(wallHand, 0)[2]]), false);
      assert.equal(await squeeze(game, wallHand), null);
      assert.equal((await climbState(game)).grace_remaining, 2, "a failed squeeze spends nothing");

      // Grace 1: the wall beside the ladder glows, then holds.
      assert.equal(await reachOpen(game, wallHand, wall(wallHand, 0.3)), true, "wall beside the held ladder glows");
      const grace1 = await squeeze(game, wallHand);
      assert.equal(grace1?.kind, "surface");
      assert.equal(grace1?.grace, 1);
      assert.equal((await climbState(game)).grace_remaining, 1);

      // Off the ladder, hanging from the wall: the body stays put.
      await open(game, ladderHand);
      let climb = await climbState(game);
      assert.equal(climb.anchor_hand, wallHand);
      assert.ok(Math.abs((await game.info()).player.position[1] - y) < 0.05, "the wall hold supports the body");

      // Grace 2, from the grace-1 hold: the wall across the ladder (1.4 away).
      assert.equal(await reachOpen(game, ladderHand, wall(ladderHand, 0.1)), true, "second surface glows");
      const grace2 = await squeeze(game, ladderHand);
      assert.equal(grace2?.kind, "surface");
      assert.equal(grace2?.grace, 2);
      assert.equal((await climbState(game)).grace_remaining, 0);

      // No third, even on the spot grace 1 took (in reach of grace hold 2):
      // neither glow nor hold, however long the hand stays closed.
      await open(game, wallHand);
      assert.equal(await reachOpen(game, wallHand, wall(wallHand, 0.3)), false, "no glow for a third");
      assert.equal(await squeeze(game, wallHand), null);
      await game.step({ frames: 30 });
      climb = await climbState(game);
      assert.equal(climb.grips.length, 1, "no third grace grab");
      assert.equal(climb.vaulting, false, "a wall hold never vaults");
    },
  );
}
