import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, HttpError } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { ledgeLadderHold } from "./helpers/vr-climb.js";
import { teleportVerified } from "./helpers/teleport.js";

// `<hand>_hand.world_target` holds a VR hand on a world point while the pawn
// moves under it. Driven at debug_ladder's ledge station, whose ladder face is
// at x ~= -6.83 (see shock2vr/src/scenes/debug_ladder.rs).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Close enough to the ladder that a rung is within arm's reach. */
const STAND: Vec3 = [-6.1, 1.5, 0];

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

async function rightHandWorld(game: GameServer): Promise<Vec3> {
  const hand = (await game.input.state()).right_hand;
  assert.ok(hand.world_position, "VR reports the hand's world position");
  return hand.world_position;
}

test(
  "debug_ladder (VR): a world_target hand stays on its point while the pawn moves",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_ladder",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });
    await teleportVerified(game, { x: STAND[0], y: STAND[1], z: STAND[2] });
    await game.step({ frames: 30 });
    const RUNG = await ledgeLadderHold(game, 2.3);
    assert.equal((await game.physics.grip(RUNG)).grip?.kind, "ladder", "the target is a rung");

    await game.input.set("right_hand.world_target", RUNG);
    await game.step({ frames: 2 });
    assert.deepEqual((await game.input.state()).right_hand.world_target, RUNG);
    assert.ok(distance(await rightHandWorld(game), RUNG) < 0.01);

    // Turn and sidestep the pawn: the hand is re-solved against the new pose.
    const before = (await game.info()).player;
    await game.input.set("left_hand.thumbstick", [1, 0]);
    await game.step({ frames: 10 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await teleportVerified(game, { x: STAND[0], y: STAND[1], z: STAND[2] + 0.3 });
    await game.step({ frames: 30 });
    const after = (await game.info()).player;
    const dot = before.rotation.reduce((sum, c, i) => sum + c * after.rotation[i], 0);
    assert.ok(Math.abs(dot) < 0.999, `the pawn turned: ${before.rotation} -> ${after.rotation}`);
    assert.ok(distance(before.position, after.position) > 0.2, "the pawn moved");
    const held = await rightHandWorld(game);
    assert.ok(distance(held, RUNG) < 0.01, `hand drifted off the rung: ${held}`);

    // Out of arm's reach: rejected in the response body, the hand left alone.
    const leftBefore = (await game.input.state()).left_hand.position;
    await assert.rejects(
      game.input.set("left_hand.world_target", [RUNG[0], RUNG[1] + 3, RUNG[2]]),
      (error: unknown) =>
        error instanceof HttpError && error.status === 400 && /arm reach/.test(error.body),
    );
    const leftAfter = await game.input.state();
    assert.deepEqual(leftAfter.left_hand.position, leftBefore);
    assert.equal(leftAfter.left_hand.world_target, null);

    // The game's own grab resolves at the targeted point.
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    const grips = (await game.info()).player.climb.grips;
    assert.equal(grips.length, 1);
    assert.equal(grips[0].kind, "ladder");
    assert.ok(distance(grips[0].point, RUNG) < 0.2, `grabbed at ${grips[0].point}`);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 30 });

    // null clears: the hand then rides with the pawn again.
    await game.input.set("right_hand.world_target", null);
    assert.equal((await game.input.state()).right_hand.world_target, null);
    await teleportVerified(game, { x: STAND[0] + 1, y: STAND[1], z: STAND[2] });
    await game.step({ frames: 30 });
    assert.ok(distance(await rightHandWorld(game), RUNG) > 0.5, "a cleared target no longer holds the hand");
  },
);
