import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { vrHandLocal, vrHandLocalDelta } from "./helpers/vr-climb.js";

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
/** Plain, non-climbable block (z = -16). */
const WALL_Z = -16;

/** Scripted top-out substeps are capped at 0.067 wu/frame. */
const MAX_SCRIPTED_STEP = 0.1;

const launchVr = () =>
  GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });

async function standAt(game: GameServer, at: Vec3) {
  await game.step({ frames: 5 });
  await teleportVerified(game, { x: at[0], y: at[1], z: at[2] });
  await game.step({ frames: 30 });
}

/** Close an open hand on a world point, and report what it took hold of. */
async function grab(
  game: GameServer,
  hand: "left" | "right",
  world: Vec3,
): Promise<Vec3> {
  const local = await vrHandLocal(game, world);
  await game.input.set(`${hand}_hand.position`, local);
  await game.input.set(`${hand}_hand.squeeze`, 0);
  await game.step({ frames: 1 });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 1 });
  return local;
}

/** Step one frame at a time, collecting the body height each frame. */
async function stepTrackingHeight(
  game: GameServer,
  frames: number,
): Promise<number[]> {
  const heights: number[] = [];
  for (let i = 0; i < frames; i += 1) {
    await game.step({ frames: 1 });
    heights.push((await game.info()).player.position[1]);
  }
  return heights;
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
    await standAt(game, [-6.35, 1.5, MANTLE_Z]);

    const atGrab = await grab(game, "right", [-7.2, 3.05, MANTLE_Z]);
    const grabbed = (await game.info()).player.climb;
    assert.equal(grabbed.grips.length, 1);
    assert.equal(grabbed.grips[0].kind, "ledge");
    assert.equal(grabbed.vaulting, false);

    // Haul down until the head clears the lip. The eye sits 1.04 wu above the
    // body center, so ~0.9 wu of pull puts it over a 3.0 lip.
    const PULL = 1.2;
    const FRAMES = 40;
    const down = await vrHandLocalDelta(game, [0, -PULL, 0]);
    const heights: number[] = [];
    let vaulted = false;
    for (let frame = 1; frame <= FRAMES && !vaulted; frame += 1) {
      await game.input.set("right_hand.position", [
        atGrab[0] + (down[0] * frame) / FRAMES,
        atGrab[1] + (down[1] * frame) / FRAMES,
        atGrab[2] + (down[2] * frame) / FRAMES,
      ]);
      heights.push(...(await stepTrackingHeight(game, 1)));
      vaulted = (await game.info()).player.climb.vaulting;
    }
    assert.ok(vaulted, "the pull should have started a vault");
    assert.equal(
      (await game.info()).player.climb.grips.length,
      0,
      "a vault lets go of everything",
    );

    // Let the scripted top-out run to its landing, then settle.
    for (let elapsed = 0; elapsed < 240; elapsed += 1) {
      heights.push(...(await stepTrackingHeight(game, 1)));
      if (!(await game.info()).player.climb.vaulting) break;
    }
    heights.push(...(await stepTrackingHeight(game, 60)));

    const landed = (await game.info()).player;
    assert.equal(landed.climb.vaulting, false);
    assert.equal(landed.climb.grips.length, 0);
    assert.ok(
      Math.abs(landed.position[1] - standingOn(MANTLE_TOP)) < 0.15,
      `expected to stand on the block (y ~= ${standingOn(MANTLE_TOP)}), got ${landed.position[1]}`,
    );
    assert.ok(
      landed.position[0] < -7.0,
      `expected to end past the block's near face, got x ${landed.position[0]}`,
    );
    assertNoJumps(heights, "mantle vault");
  },
);

for (const transferHeight of [4.5, 5.1]) {
for (const twoDeckHands of [false, true]) {
test(
  `debug_ladder (VR): ${twoDeckHands ? "two deck hands" : "a deck grab"} at body height ${transferHeight} vaults off the ladder`,
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAt(game, [-6.35, 1.5, LEDGE_Z]);

    // Hand over hand up the near-face ladder. Hands are tracked in PAWN space,
    // so one reach height serves every cycle - the same reach lands on a
    // higher rung as the body rises.
    const reach = await vrHandLocal(game, [-6.8, 2.6, LEDGE_Z]);
    const PULL = 0.5;
    const FRAMES = 10;
    const down = await vrHandLocalDelta(game, [0, -PULL, 0]);
    const lowered: Vec3 = [
      reach[0] + down[0],
      reach[1] + down[1],
      reach[2] + down[2],
    ];

    let pulling: "left" | "right" = "right";
    await grab(game, pulling, [-6.8, 2.6, LEDGE_Z]);
    for (let half = 0; half < 8; half += 1) {
      if ((await game.info()).player.position[1] > transferHeight) break;
      for (let frame = 1; frame <= FRAMES; frame += 1) {
        await game.input.set(`${pulling}_hand.position`, [
          reach[0] + (down[0] * frame) / FRAMES,
          reach[1] + (down[1] * frame) / FRAMES,
          reach[2] + (down[2] * frame) / FRAMES,
        ]);
        await game.step({ frames: 1 });
      }
      if ((await game.info()).player.position[1] > transferHeight) break;
      const other: "left" | "right" = pulling === "right" ? "left" : "right";
      await game.input.set(`${other}_hand.position`, reach);
      await game.input.set(`${other}_hand.squeeze`, 0);
      await game.step({ frames: 1 });
      await game.input.set(`${other}_hand.squeeze`, 1);
      await game.step({ frames: 1 });
      await game.input.set(`${pulling}_hand.squeeze`, 0);
      await game.input.set(`${pulling}_hand.position`, lowered);
      await game.step({ frames: 1 });
      pulling = other;
    }
    const climbed = (await game.info()).player;
    assert.ok(
      climbed.position[1] > transferHeight,
      `hand over hand only reached ${climbed.position[1]}`,
    );
    assert.equal(climbed.climb.grips[0].kind, "ladder");
    assert.equal(climbed.climb.vaulting, false, "a ladder rail never vaults");

    // Reach onto the deck AFTER the head clears it: this used to permanently
    // fail the historical eye-at-grab gate. The new grip still waits for a pull.
    let other: "left" | "right" = pulling === "right" ? "left" : "right";
    let onTop = await grab(game, other, [-7.1, 6.05, LEDGE_Z]);
    const held = (await game.info()).player.climb;
    assert.equal(held.anchor_hand, other);
    assert.equal(held.vaulting, false, "resting the hand on the deck is not a vault");
    assert.equal(
      held.grips.find((grip) => grip.hand === other)?.kind,
      "ledge",
      "the block's top surface is a ledge hold",
    );

    if (twoDeckHands) {
      // Move the old ladder hand onto the same deck while the first deck
      // hand supports us. Then release that first hand: either deck hand
      // must support a slow pull, without a release flick.
      await game.input.set(`${pulling}_hand.squeeze`, 0);
      await game.step({ frames: 1 });
      const second = await grab(game, pulling, [-7.1, 6.05, LEDGE_Z + 0.3]);
      const atSecondGrip = (await game.info()).player;
      const both = atSecondGrip.climb;
      assert.equal(both.grips.length, 2);
      assert.ok(both.grips.every((grip) => grip.kind === "ledge"));
      await game.input.set(`${other}_hand.squeeze`, 0);
      await game.step({ frames: 30 });
      const supported = (await game.info()).player;
      assert.equal(supported.climb.grips.length, 1);
      assert.equal(supported.climb.anchor_hand, pulling);
      assert.ok(Math.abs(supported.position[1] - atSecondGrip.position[1]) < 0.01,
        `the second deck hand must prevent falling while resting: before=${atSecondGrip.position}, after=${supported.position}, climb=${JSON.stringify(supported.climb)}`);
      other = pulling;
      onTop = second;
    }

    const topOut = await vrHandLocalDelta(game, [0, -0.8, 0]);
    const heights: number[] = [];
    let vaulted = false;
    for (let frame = 1; frame <= 40 && !vaulted; frame += 1) {
      await game.input.set(`${other}_hand.position`, [
        onTop[0] + (topOut[0] * frame) / 40,
        onTop[1] + (topOut[1] * frame) / 40,
        onTop[2] + (topOut[2] * frame) / 40,
      ]);
      heights.push(...(await stepTrackingHeight(game, 1)));
      vaulted = (await game.info()).player.climb.vaulting;
    }
    assert.ok(vaulted, "the eye clearing 6.0 should have started a vault");

    for (let elapsed = 0; elapsed < 240; elapsed += 1) {
      heights.push(...(await stepTrackingHeight(game, 1)));
      if (!(await game.info()).player.climb.vaulting) break;
    }
    heights.push(...(await stepTrackingHeight(game, 60)));

    const landed = (await game.info()).player;
    assert.equal(landed.climb.grips.length, 0);
    assert.ok(
      Math.abs(landed.position[1] - standingOn(LEDGE_TOP)) < 0.15,
      `expected to stand on the block (y ~= ${standingOn(LEDGE_TOP)}), got ${landed.position[1]}`,
    );
    assertNoJumps(heights, "ledge vault");
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
    await standAt(game, [-6.35, 1.5, WALL_Z]);
    await grab(game, "right", [-6.9, 2.6, WALL_Z]);
    let climb = (await game.info()).player.climb;
    assert.equal(climb.grips.length, 0);
    assert.equal(climb.vaulting, false);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 1 });

    // On the ledge ladder with the eye far below the block top, pulling climbs
    // and nothing else.
    await standAt(game, [-6.35, 1.5, LEDGE_Z]);
    const atGrab = await grab(game, "right", [-6.8, 2.6, LEDGE_Z]);
    const down = await vrHandLocalDelta(game, [0, -1.0, 0]);
    for (let frame = 1; frame <= 30; frame += 1) {
      await game.input.set("right_hand.position", [
        atGrab[0] + (down[0] * frame) / 30,
        atGrab[1] + (down[1] * frame) / 30,
        atGrab[2] + (down[2] * frame) / 30,
      ]);
      await game.step({ frames: 1 });
      assert.equal(
        (await game.info()).player.climb.vaulting,
        false,
        `vaulted at frame ${frame} with the eye far below the lip`,
      );
    }
    climb = (await game.info()).player.climb;
    assert.equal(climb.grips.length, 1);
    assert.equal(climb.grips[0].kind, "ladder");
  },
);

test(
  "debug_ladder (VR): letting go of a ledge before the vault just drops you",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await launchVr();
    await standAt(game, [-6.35, 1.5, MANTLE_Z]);
    const floorY = (await game.info()).player.position[1];

    const atGrab = await grab(game, "right", [-7.2, 3.05, MANTLE_Z]);
    // A short, slow pull: the eye stays under the lip the whole way.
    const down = await vrHandLocalDelta(game, [0, -0.3, 0]);
    for (let frame = 1; frame <= 40; frame += 1) {
      await game.input.set("right_hand.position", [
        atGrab[0] + (down[0] * frame) / 40,
        atGrab[1] + (down[1] * frame) / 40,
        atGrab[2] + (down[2] * frame) / 40,
      ]);
      await game.step({ frames: 1 });
      assert.equal((await game.info()).player.climb.vaulting, false);
    }

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 90 });
    const dropped = (await game.info()).player;
    assert.equal(dropped.climb.vaulting, false, "a release is not a vault");
    assert.equal(dropped.climb.grips.length, 0);
    assert.ok(
      Math.abs(dropped.position[1] - floorY) < 0.2,
      `expected a fall back to the floor (${floorY}), got ${dropped.position[1]}`,
    );
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
  const start = await grab(game, "right", [-6.9, 6.45, LEDGE_Z]);
  const caught = (await game.info()).player;
  assert.equal(caught.climb.grips.length, 1, "catch from above the cap");
  assert.equal(caught.climb.grips[0].kind, "ladder");
  assert.ok(Math.abs(caught.position[0] - before[0]) < 0.05, "catch must not snap the body");
  const hook = (await game.physics.grip([-6.9, 6.45, LEDGE_Z])).grip!;
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
