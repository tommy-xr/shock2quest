import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, otherHand, vrClimbLadder, vrGrab, vrPull, vrTopOut, vrVault } from "../src/index.js";
import type { Hand, Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { ledgeLadderHold, vrHandLocalDelta } from "./helpers/vr-climb.js";

// Finishing the mantle: a hand on a ledge gets the body onto it. Once the eye
// clears the lip the hand is lying on, the grips let go and the scripted
// top-out throws the body over. Driven on the debug_ladder scene (see
// shock2vr/src/scenes/debug_ladder.rs).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Capsule-center height of a player standing on a surface at height `top`. */
const standingOn = (top: number) => top + 1.24;

/** Mantle station (z = 24): a 3 wu block with no ladder. */
const MANTLE_Z = 24;
const MANTLE_TOP = 3.0;
/** Ledge station (z = 0): a 6 wu block with a 16' ladder on its near face. */
const LEDGE_Z = 0;
const LEDGE_TOP = 6.0;
/** Stacked-rung station (z = -8): a 9 wu wall with one ladder entity per rung. */
const STACK_Z = -8;
const STACK_TOP = 9.0;
/** Plain, non-climbable block (z = -16). */
const WALL_Z = -16;

/** A standing body (radius 0.48) touching the station faces at x -7.0. A
 * start any closer is inside the block, where standing back up is refused. */
const FLUSH_X = -6.48;
/** Against the mantle block, and the lip a standing arm can just hook. */
const MANTLE_STAND: Vec3 = [FLUSH_X, 1.5, MANTLE_Z];
/** Toward the right shoulder (-z), which keeps it inside the 0.7 m reach. */
const MANTLE_LIP: Vec3 = [-6.9, 2.75, MANTLE_Z - 0.15];
/** Against the ledge ladder, and a rung within reach from the floor. */
const LEDGE_STAND: Vec3 = [FLUSH_X, 1.5, LEDGE_Z];
/** Deck holds from the ledge ladder: just past the lip, and as deep as an arm
 * on the ladder reaches (about 0.35 wu). */
const DECK_LIP: Vec3 = [-7.05, 6.05, LEDGE_Z];
const DECK_MID: Vec3 = [-7.3, 6.05, LEDGE_Z];


/** Scripted top-out substeps are capped at 0.067 wu/frame. */
const MAX_SCRIPTED_STEP = 0.1;

const launchVr = () =>
  GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });

async function standAt(game: GameServer, at: Vec3) {
  await game.step({ frames: 5 });
  await teleportVerified(game, { x: at[0], y: at[1], z: at[2] });
  await game.step({ frames: 30 });
}

function assertNoJumps(heights: number[], label: string) {
  for (let i = 1; i < heights.length; i += 1) {
    assert.ok(
      Math.abs(heights[i] - heights[i - 1]) < MAX_SCRIPTED_STEP,
      `${label}: the body jumped ${heights[i - 1]} -> ${heights[i]} in one frame`,
    );
  }
}

test(
  "debug_ladder (VR): pulling the eye over a mantle lip vaults onto the block",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAt(game, MANTLE_STAND);

    // The block top is above a standing arm's reach; its lip is not. Haul
    // down until the head clears the lip, which vaults and lets go.
    const { heights, landed } = await vrTopOut(game, "right", MANTLE_LIP);

    const climb = (await game.info()).player.climb;
    assert.equal(climb.vaulting, false);
    assert.equal(climb.grips.length, 0, "a vault lets go of everything");
    assert.ok(
      Math.abs(landed[1] - standingOn(MANTLE_TOP)) < 0.15,
      `expected to stand on the block (y ~= ${standingOn(MANTLE_TOP)}), got ${landed[1]}`,
    );
    assert.ok(
      landed[0] < -7.0,
      `expected to end past the block's near face, got x ${landed[0]}`,
    );
    assertNoJumps(heights, "mantle vault");
  },
);

/** Climb the ledge ladder hand over hand until the body centre is at `height`. */
async function climbLedgeLadderTo(game: GameServer, height: number): Promise<Hand> {
  await standAt(game, LEDGE_STAND);
  const { anchor } = await vrClimbLadder(game, { near: LEDGE_RUNG, untilY: height });
  const climbed = (await game.info()).player;
  assert.ok(climbed.position[1] >= height, `hand over hand only reached ${climbed.position[1]}`);
  assert.equal(climbed.climb.grips[0].kind, "ladder");
  assert.equal(climbed.climb.vaulting, false, "a ladder rail never vaults");
  return anchor;
}

async function assertLandedOnLedge(game: GameServer, heights: number[], label: string) {
  const landed = (await game.info()).player;
  assert.equal(landed.climb.grips.length, 0);
  assert.ok(
    Math.abs(landed.position[1] - standingOn(LEDGE_TOP)) < 0.15,
    `expected to stand on the block (y ~= ${standingOn(LEDGE_TOP)}), got ${landed.position[1]}`,
  );
  assertNoJumps(heights, label);
}

// The ledge ladder (one 16' entity) and the stacked-rung wall (one entity per
// rung, 9 wu): climb to just under the top, then take the deck at its lip or
// as deep as the arm reaches.
for (const [station, z, top] of [["ledge", LEDGE_Z, LEDGE_TOP], ["stacked-rung", STACK_Z, STACK_TOP]] as const) {
  for (const [name, depth] of [["lip", DECK_LIP[0]], ["mid-deck", DECK_MID[0]]] as const) {
    test(
      `debug_ladder (VR): climbing the ${station} ladder tops out by a ${name} grab`,
      { skip: !e2eEnabled, timeout: 600_000 },
      async () => {
        await using game = await launchVr();
        await standAt(game, [LEDGE_STAND[0], LEDGE_STAND[1], z]);
        const { anchor } = await vrClimbLadder(game, { near: [LEDGE_RUNG[0], LEDGE_RUNG[1], z], untilY: top - 0.9 });
        const { heights, landed } = await vrTopOut(game, otherHand(anchor), [depth, top + 0.05, z]);
        assert.equal((await game.info()).player.climb.grips.length, 0);
        assert.ok(
          Math.abs(landed[1] - standingOn(top)) < 0.15,
          `expected to stand on the ${station} top (y ~= ${standingOn(top)}), got ${landed[1]}`,
        );
        assertNoJumps(heights, `${station} ${name} top-out`);
      },
    );
  }
}

// 4.9 grabs the deck with the eye still under it, 5.1 after it clears.
for (const transferHeight of [4.9, 5.1]) {
for (const twoDeckHands of [false, true]) {
test(
  `debug_ladder (VR): ${twoDeckHands ? "two deck hands" : "a deck grab"} at body height ${transferHeight} vaults off the ladder`,
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    const ladderHand = await climbLedgeLadderTo(game, transferHeight);

    // Reach onto the deck: the grip waits for a pull.
    let deckHand = otherHand(ladderHand);
    await vrGrab(game, deckHand, DECK_LIP);
    const held = (await game.info()).player.climb;
    assert.equal(held.anchor_hand, deckHand);
    assert.equal(held.vaulting, false, "resting the hand on the deck is not a vault");
    assert.equal(
      held.grips.find((grip) => grip.hand === deckHand)?.kind,
      "ledge",
      "the block's top surface is a ledge hold",
    );

    if (twoDeckHands) {
      // Move the ladder hand onto the same deck while the first deck hand
      // supports us. Then release that first hand: either deck hand must
      // support the body, without a release flick.
      await game.input.set(`${ladderHand}_hand.squeeze`, 0);
      await game.step({ frames: 1 });
      await vrGrab(game, ladderHand, [DECK_LIP[0], DECK_LIP[1], DECK_LIP[2] + 0.3]);
      const atSecondGrip = (await game.info()).player;
      const both = atSecondGrip.climb;
      assert.equal(both.grips.length, 2);
      assert.ok(both.grips.every((grip) => grip.kind === "ledge"));
      await game.input.set(`${deckHand}_hand.squeeze`, 0);
      await game.step({ frames: 30 });
      const supported = (await game.info()).player;
      assert.equal(supported.climb.grips.length, 1);
      assert.equal(supported.climb.anchor_hand, ladderHand);
      assert.ok(Math.abs(supported.position[1] - atSecondGrip.position[1]) < 0.01,
        `the second deck hand must prevent falling while resting: before=${atSecondGrip.position}, after=${supported.position}, climb=${JSON.stringify(supported.climb)}`);
      deckHand = ladderHand;
    }

    // Pull on whichever deck hand holds on.
    const { heights } = await vrVault(game, deckHand);
    await assertLandedOnLedge(game, heights, "ledge vault");
  },
);

}
}

test(
  "debug_ladder (VR): a plain wall and a low eye never vault",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();

    // The plain wall offers no hold at all, so there is nothing to vault from.
    await standAt(game, [LEDGE_STAND[0], LEDGE_STAND[1], WALL_Z]);
    await assert.rejects(vrGrab(game, "right", [-6.9, 2.3, WALL_Z]), /closed on nothing/);
    let climb = (await game.info()).player.climb;
    assert.equal(climb.grips.length, 0);
    assert.equal(climb.vaulting, false);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });

    // On the ledge ladder with the eye far below the block top, pulling climbs
    // and nothing else.
    await standAt(game, LEDGE_STAND);
    await vrGrab(game, "right", await ledgeLadderHold(game, 2.0));
    const path = await vrPull(game, "right", [0, -1.0, 0], 30, (p) => p.climb.vaulting);
    assert.equal(path.length, 30, `vaulted at frame ${path.length} with the eye far below the lip`);
    climb = (await game.info()).player.climb;
    assert.equal(climb.vaulting, false);
    assert.equal(climb.grips.length, 1);
    assert.equal(climb.grips[0].kind, "ladder");
  },
);

test(
  "debug_ladder (VR): letting go of a ledge before the vault just drops you",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAt(game, MANTLE_STAND);
    const floorY = (await game.info()).player.position[1];

    await vrGrab(game, "right", MANTLE_LIP);
    // A short, slow pull: the eye stays under the lip the whole way.
    const path = await vrPull(game, "right", [0, -0.3, 0], 40, (p) => p.climb.vaulting);
    assert.equal(path.length, 40, "a short pull must not vault");

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 90 });
    const dropped = (await game.info()).player;
    assert.equal(dropped.climb.vaulting, false, "a release is not a vault");
    assert.equal(dropped.climb.grips.length, 0);
    // Back on the floor.
    assert.ok(
      dropped.position[1] <= floorY + 0.05,
      `expected a fall back to the floor (${floorY}), got ${dropped.position[1]}`,
    );
  },
);

test(
  "debug_ladder (VR): crouch-walking into the mantle block still stands back up",
  // Crouching keeps the standing width, so a crouched body can walk no
  // closer to the block than a standing one fits.
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAt(game, MANTLE_STAND);
    const standing = (await game.info()).player.position[1];
    await game.input.set("crouch", 1);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.input.set("crouch", 0);
    await game.step({ frames: 30 });
    const y = (await game.info()).player.position[1];
    assert.ok(Math.abs(y - standing) < 0.05, `expected to stand back up to ${standing}, got ${y}`);
  },
);

test("debug_ladder (VR): crouch on the deck, hook the ladder cap and descend", {
  skip: !e2eEnabled, timeout: 600_000,
}, async () => {
  await using game = await launchVr();
  await standAt(game, [-7.65, 7.3, LEDGE_Z]);
  await game.input.set("crouch", 1);
  await game.step({ frames: 5 });
  const before = (await game.info()).player.position;
  const cap = await ledgeLadderHold(game, 6.3, true);
  await vrGrab(game, "right", cap);
  const start = (await game.input.state()).right_hand.position;
  const caught = (await game.info()).player;
  assert.equal(caught.climb.grips.length, 1, "catch from above the cap");
  assert.equal(caught.climb.grips[0].kind, "ladder");
  assert.ok(Math.abs(caught.position[0] - before[0]) < 0.05, "catch must not snap the body");
  const hook = (await game.physics.grip(cap)).grip!;
  assert.ok(Math.abs(hook.normal[0]) > 0.9 && Math.abs(hook.normal[1]) < 0.01,
    `catch an actual side, not the masked cap: ${hook.normal}`);

  // Raising the hand while feet are still on the deck cannot pass through
  // that deck. The persistent grip (and its marker) distinguishes this from
  // a failed acquisition.
  const raised = await vrHandLocalDelta(game, [0, 0.2, 0]);
  for (let i = 1; i <= 12; i++) {
    await game.input.set("right_hand.position", start.map((v, j) => v + raised[j] * i / 12) as Vec3);
    await game.step({ frames: 1 });
  }
  const blocked = (await game.info()).player;
  assert.equal(blocked.climb.grips.length, 1);
  assert.ok(Math.abs(blocked.position[1] - before[1]) < 0.05, "deck still supports the feet");
  await game.input.set("right_hand.position", start);
  await game.step({ frames: 1 });

  // Pull toward the chest first, moving the body beyond the deck edge; then
  // raise the held hand to lower the body. Every frame must retain the hold.
  const outward = await vrHandLocalDelta(game, [-1.3, 0, 0]);
  const lower = await vrHandLocalDelta(game, [0, 0.6, 0]);
  const heights: number[] = [];
  for (let phase = 0; phase < 2; phase++) {
    for (let i = 1; i <= 60; i++) {
      const t = i / 60;
      await game.input.set("right_hand.position", start.map((v, j) =>
        v + outward[j] * (phase === 0 ? t : 1) + lower[j] * (phase === 1 ? t : 0),
      ) as Vec3);
      await game.step({ frames: 1 });
      const player = (await game.info()).player;
      assert.equal(player.climb.grips.length, 1, `hold survives phase ${phase} frame ${i}`);
      assert.equal(player.climb.vaulting, false, "descent stays hand-directed");
      heights.push(player.position[1]);
    }
  }
  const descended = (await game.info()).player.position;
  assert.ok(descended[0] > -6.5, "body moved outside the deck edge");
  assert.ok(descended[1] < before[1] - 0.5, "body lowered onto the ladder");
  assertNoJumps(heights, "deck-to-ladder descent");
  await game.step({ frames: 10 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.climb.grips.length, 0, "opening the hand releases the catch");
});
