import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, RayCastResult } from "../src/index.js";

// End-to-end test for flat (desktop-style) ladder climbing.
//
// medsci1's critical path starts with a REQUIRED ladder climb out of the cryo
// recovery bay (rung entities named "Rick Ladder", PropPhysAttr.climbable=27,
// stacked at x=-41.3, z=16.5). A player pressing forward into the ladder must
// ascend it - Half-Life style - instead of being stopped by the ladder's
// collider.
//
// Negative-first: without the climbing implementation the forward input just
// presses the player against the ladder collider (y stays at floor height), so
// the ascent assertion fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

type LadderColumn = { x: number; z: number; ys: number[] };

function groupLadderColumns(rungs: EntitySummary[]): LadderColumn[] {
  const columns = new Map<string, LadderColumn>();
  for (const rung of rungs) {
    const [x, y, z] = rung.position;
    const key = `${Math.round(x)},${Math.round(z)}`;
    const column = columns.get(key) ?? { x, z, ys: [] };
    column.ys.push(y);
    columns.set(key, column);
  }
  return [...columns.values()];
}

async function findRick1OpeningLadder(
  game: GameServer,
): Promise<LadderColumn> {
  const rungs = (
    await game.entities.list({ filter: "Rick Ladder 16", limit: 100 })
  ).entities;
  assert.ok(rungs.length > 0, "rick1 should contain Rick Ladder 16 entities");

  const spawn = await game.player.position();
  const fullHeightColumns = groupLadderColumns(rungs).filter(
    (col) => Math.max(...col.ys) - Math.min(...col.ys) > 15,
  );
  assert.ok(
    fullHeightColumns.length > 0,
    "rick1 should contain a full-height ladder stack",
  );
  return fullHeightColumns.reduce((best, col) => {
    const distance = (candidate: LadderColumn) =>
      Math.hypot(candidate.x - spawn.x, candidate.z - spawn.z);
    return distance(col) < distance(best) ? col : best;
  });
}

test(
  "flat climbing: pushing into a ladder ascends it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    // Resolve the cryo-recovery ladder at run time (runtime entity ids are not
    // stable across launches): cluster the "Rick Ladder" rungs by column and
    // take the column near (-17.5, 14.5) - the shaft ladder on medsci1's
    // critical path. (The other cryo-bay column, at (-41.3, 16.5), is fenced
    // off by the fallen air duct the player is meant to wrench-smash first, so
    // it can't be walked up to.)
    const rungs = (await game.entities.list({ filter: "Rick Ladder", limit: 100 }))
      .entities;
    assert.ok(rungs.length > 0, "medsci1 should contain Rick Ladder entities");

    const ladder = groupLadderColumns(rungs).reduce((best, col) => {
      const d = (c: LadderColumn) =>
        Math.hypot(c.x - -17.5, c.z - 14.5);
      return d(col) < d(best) ? col : best;
    });
    const ladderTop = Math.max(...ladder.ys);

    // Stand on the bay floor just north (+z) of the ladder plane. The rungs
    // face +z there (the -z side drops into a deep shaft), and with the debug
    // runtime's default camera (facing -x) a LEFT strafe moves the player -z,
    // INTO the ladder face (mapping verified empirically).
    await game.player.teleport({
      x: ladder.x,
      y: -4.5,
      z: ladder.z + 1.5,
    });
    await game.step({ frames: 30 }); // settle onto the floor
    const before = await game.player.position();
    assert.ok(
      before.y < ladderTop,
      `expected to start below the ladder top (${ladderTop}), got y=${before.y}`,
    );

    // Press into the ladder and hold.
    await game.input.set("right_hand.thumbstick", [-1, 0]);
    await game.step({ frames: 240 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const after = await game.player.position();
    const ascent = after.y - before.y;
    assert.ok(
      ascent > 2,
      `pressing into the ladder should climb it (started y=${before.y.toFixed(2)}, ` +
        `ended y=${after.y.toFixed(2)}, ascent=${ascent.toFixed(2)}; ` +
        "without climbing the ladder collider just blocks the player)",
    );
  },
);

// Issue #626: rick1's opening ladder extends above the upper-deck landing.
// Vertical-only climbing reaches the cap at about (20.4, 19.2, 2.0), but
// ordinary collision then keeps the standing capsule on the shaft side. This
// regression holds the same forward input through the transition and requires
// the production movement path to finish on the upper deck.
test(
  "flat climbing: forward input tops out onto rick1's opening deck",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rick1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    // Runtime ids change every launch. Resolve the authored full-height stack
    // nearest the fresh arrival-room spawn.
    const ladder = await findRick1OpeningLadder(game);
    const ladderBottom = Math.min(...ladder.ys);
    const ladderTop = Math.max(...ladder.ys);

    // Stage on the arrival-room floor facing slightly toward the deck side of
    // the ladder. The climb and top-out themselves use only ordinary
    // locomotion; no direct relocation or entity message occurs during them.
    await game.player.teleport({
      x: ladder.x - 1,
      y: ladderBottom - 2.2,
      z: ladder.z - 0.4,
    });
    await game.step({ frames: 30 });
    await game.input.lookAtWorldPoint([
      ladder.x + 8,
      ladderTop + 2,
      ladder.z + 2,
    ]);

    // Climb with the slight +Z heading, stopping below the top-out range.
    // The original-height body rests slightly lower on this irregular rung
    // stack than the temporary capsule did, so keep a broad sub-frame staging
    // band while remaining well below the actual top-out threshold.
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let beforeTopOut = await game.player.position();
    for (let elapsed = 0; elapsed < 720; ) {
      const frames = beforeTopOut.y < ladderTop - 3 ? 30 : 1;
      await game.step({ frames });
      elapsed += frames;
      beforeTopOut = await game.player.position();
      if (beforeTopOut.y >= ladderTop - 2.7) {
        break;
      }
    }
    assert.ok(
      beforeTopOut.y >= ladderTop - 2.7 && beforeTopOut.y < ladderTop - 2.4,
      `forward input should reach the pre-top-out approach; ladder top=${ladderTop.toFixed(2)}, ` +
        `ladder=(${ladder.x.toFixed(2)}, ${ladder.z.toFixed(2)}), ` +
        `ended=(${beforeTopOut.x.toFixed(2)}, ${beforeTopOut.y.toFixed(2)}, ${beforeTopOut.z.toFixed(2)})`,
    );

    // Aim diagonally +Z BEFORE the top-out can be planned. The z=2.4 level wall
    // is genuinely solid along that route: the ordered transition must rise
    // while remaining on its near side, then cross only after clearing it.
    const topOutTarget = {
      x: ladder.x + 8,
      z: ladder.z + 4,
    };
    await game.input.lookAtWorldPoint([
      topOutTarget.x,
      ladderTop + 2,
      topOutTarget.z,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    const topOutLength = Math.hypot(
      topOutTarget.x - beforeTopOut.x,
      topOutTarget.z - beforeTopOut.z,
    );
    const topOutDirection = {
      x: (topOutTarget.x - beforeTopOut.x) / topOutLength,
      z: (topOutTarget.z - beforeTopOut.z) / topOutLength,
    };
    let lastBeforeForward = beforeTopOut;
    let firstForward = beforeTopOut;
    for (let elapsed = 0; elapsed < 360; elapsed += 1) {
      const previous = firstForward;
      await game.step({ frames: 1 });
      firstForward = await game.player.position();
      const forwardDelta =
        (firstForward.x - previous.x) * topOutDirection.x +
        (firstForward.z - previous.z) * topOutDirection.z;
      if (forwardDelta > 0.001) {
        lastBeforeForward = previous;
        break;
      }
    }
    assert.ok(
      lastBeforeForward.z < ladder.z - 0.3 &&
        lastBeforeForward.y > ladderTop,
      `the top-out must reach clearance height on the near side before its first forward crossing step; ` +
        `approach=(${beforeTopOut.y.toFixed(2)}, ${beforeTopOut.z.toFixed(2)}), ` +
        `last-before-forward=(${lastBeforeForward.y.toFixed(2)}, ${lastBeforeForward.z.toFixed(2)}), ` +
        `first-forward=(${firstForward.y.toFixed(2)}, ${firstForward.z.toFixed(2)})`,
    );

    // Keep the same ordinary forward input held. The scripted rise must clear
    // the wall before the crossing, then fully expand and restore ordinary
    // walking beyond the ladder's geometry-derived exit. Scripted mantle
    // substeps are capped at 0.067 world units/frame; the first >0.1 projected
    // frame therefore proves the standing capsule and regular locomotion were
    // restored before input release.
    const cap = lastBeforeForward;
    let cleared = firstForward;
    let previousHeld = firstForward;
    let regularWalkResumed = false;
    for (let elapsed = 0; elapsed < 360; elapsed += 1) {
      await game.step({ frames: 1 });
      cleared = await game.player.position();
      const forwardDelta =
        (cleared.x - previousHeld.x) * topOutDirection.x +
        (cleared.z - previousHeld.z) * topOutDirection.z;
      const beyondLip =
        cleared.x > ladder.x + 1 &&
        cleared.z > ladder.z + 0.35;
      regularWalkResumed = beyondLip && forwardDelta > 0.1;
      previousHeld = cleared;
      if (regularWalkResumed) {
        break;
      }
    }
    assert.ok(
      regularWalkResumed,
      `the held forward input must complete the standing top-out before release; ` +
        `ended=(${cleared.x.toFixed(2)}, ${cleared.y.toFixed(2)}, ${cleared.z.toFixed(2)})`,
    );
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });
    const landed = await game.player.position();
    await game.step({ frames: 60 });
    const stable = await game.player.position();

    assert.ok(
      stable.x > ladder.x + 1 &&
        stable.z > ladder.z + 0.35 &&
        stable.y > ladderTop - 1.5 &&
        stable.y < ladderTop + 0.2 &&
        Math.abs(stable.y - landed.y) < 0.1,
      `diagonal forward input should top out onto the lower deck beyond the rail; ` +
        `ladder=(${ladder.x.toFixed(2)}, ${ladder.z.toFixed(2)}), ` +
        `cap=(${cap.x.toFixed(2)}, ${cap.y.toFixed(2)}, ${cap.z.toFixed(2)}), ` +
        `cleared=(${cleared.x.toFixed(2)}, ${cleared.y.toFixed(2)}, ${cleared.z.toFixed(2)}), ` +
        `landed=(${landed.x.toFixed(2)}, ${landed.y.toFixed(2)}, ${landed.z.toFixed(2)}), ` +
        `stable=(${stable.x.toFixed(2)}, ${stable.y.toFixed(2)}, ${stable.z.toFixed(2)})`,
    );

    // Prove the result is a usable deck, not another stable rail perch: walk
    // farther toward the first egg's +Z side, then turn +X and advance into the
    // room using only ordinary locomotion.
    await game.input.lookAtWorldPoint([
      stable.x,
      stable.y + 1.6,
      ladder.z + 4,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let deckSide = stable;
    for (let elapsed = 0; elapsed < 60; elapsed += 2) {
      await game.step({ frames: 2 });
      deckSide = await game.player.position();
      if (deckSide.z > stable.z + 0.75) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });

    await game.input.lookAtWorldPoint([
      deckSide.x + 8,
      deckSide.y + 1.6,
      deckSide.z,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let advanced = deckSide;
    for (let elapsed = 0; elapsed < 120; elapsed += 5) {
      await game.step({ frames: 5 });
      advanced = await game.player.position();
      if (advanced.x > deckSide.x + 1.5) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 60 });
    const final = await game.player.position();

    assert.ok(
      final.x > ladder.x + 3 &&
        final.z > ladder.z + 1 &&
        deckSide.z > stable.z + 0.5 &&
        final.x > deckSide.x + 1 &&
        final.y > ladderTop - 1.5 &&
        final.y < ladderTop + 0.2,
      `the top-out landing should support ordinary onward deck movement; ` +
        `stable=(${stable.x.toFixed(2)}, ${stable.y.toFixed(2)}, ${stable.z.toFixed(2)}), ` +
        `deckSide=(${deckSide.x.toFixed(2)}, ${deckSide.y.toFixed(2)}, ${deckSide.z.toFixed(2)}), ` +
        `ended=(${final.x.toFixed(2)}, ${final.y.toFixed(2)}, ${final.z.toFixed(2)})`,
    );
  },
);

// Issue #657: a single reasonable +X,+Z heading held continuously from the
// base approached the ladder on a path whose climb was blocked 3.1 feet below
// the authored column top. The original shorter top-out gate never planned a
// mantle; after moving-terrain support landed, the same route could instead
// leave scripted climbing through unsupported ordinary movement. Reproduce the
// player's one-heading approach and require a controlled, supported release.
test(
  "flat climbing: continuous diagonal heading tops out onto rick1's opening deck",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rick1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    const ladder = await findRick1OpeningLadder(game);
    const ladderBottom = Math.min(...ladder.ys);
    const ladderTop = Math.max(...ladder.ys);

    // Start in clear floor space on the west/south side. The old direct-contact
    // teleport was valid for the temporary narrow player but overlaps this
    // rung stack after #712 restores Dark's 2.4-foot standing footprint. From
    // this reachable pose, one continuous production heading performs the
    // approach, grip, climb, and top-out. Aim in world space because the
    // low-level `head.look` channel is pawn-local and rick1's restored player
    // rotation otherwise turns the nominal +X,+Z heading toward -Z.
    const clearTarget = {
      x: ladder.x - 1,
      y: ladderBottom - 2.2,
      z: ladder.z - 0.4,
    };
    await game.player.teleport(clearTarget);
    await game.step({ frames: 30 });
    const base = await game.player.position();
    const baseSupport = await game.raycast({
      start: [base.x, base.y, base.z],
      end: [base.x, base.y - 3, base.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      Math.hypot(
        base.x - clearTarget.x,
        base.z - clearTarget.z,
      ) < 0.05 &&
        baseSupport.hit &&
        baseSupport.distance !== null &&
        baseSupport.distance > 0.5 &&
        baseSupport.distance < 1.5 &&
        baseSupport.hit_normal !== null &&
        baseSupport.hit_normal[1] > 0.5,
      `expected to settle in clear floor space before the one-heading approach; ` +
        `ladder=(${ladder.x.toFixed(2)}, ${ladder.z.toFixed(2)}), ` +
        `target=(${clearTarget.x.toFixed(2)}, ${clearTarget.y.toFixed(2)}, ${clearTarget.z.toFixed(2)}), ` +
        `base=(${base.x.toFixed(2)}, ${base.y.toFixed(2)}, ${base.z.toFixed(2)}), ` +
        `support=${JSON.stringify(baseSupport)}`,
    );

    await game.input.lookAtWorldPoint([
      ladder.x + 8,
      ladderTop + 2,
      ladder.z + 2,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    let previous = base;
    let position = base;
    let supportedFarSideReached = false;
    let ordinaryWalkStart = base;
    let ordinaryWalkSupport = "";
    for (let elapsed = 0; elapsed < 1_050; ) {
      // Sample every frame from input onset. With the restored full footprint,
      // the bounded mantle can begin several feet below the authored rung top;
      // a coarse ascent batch can therefore hide the first supported far-side
      // landing pose that this safety regression must inspect.
      const frames = 1;
      await game.step({ frames });
      elapsed += frames;
      previous = position;
      position = await game.player.position();
      const beyondLip =
        position.x > ladder.x && position.z > ladder.z + 0.35;
      const atDeckHeight =
        position.y > ladderTop - 1.5 && position.y < ladderTop + 0.2;
      if (beyondLip && atDeckHeight) {
        const nearbySupport = await game.raycast({
          start: [position.x, position.y, position.z],
          end: [position.x, position.y - 3, position.z],
          collision_groups: ["world", "entity", "selectable"],
          ignore_sensors: true,
        });
        const hasNearbySupport =
          nearbySupport.hit &&
          nearbySupport.distance !== null &&
          nearbySupport.distance > 0.5 &&
          nearbySupport.distance < 1.25 &&
          nearbySupport.hit_normal !== null &&
          nearbySupport.hit_normal[1] > 0.5;
        if (hasNearbySupport) {
          supportedFarSideReached = true;
          ordinaryWalkStart = position;
          ordinaryWalkSupport = JSON.stringify(nearbySupport);
        }
      }
      if (supportedFarSideReached) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);

    assert.ok(
      supportedFarSideReached,
      `one continuous diagonal heading must reach supported standing height past the lip; ` +
        `ladder=(${ladder.x.toFixed(2)}, ${ladder.z.toFixed(2)}), ` +
        `base=(${base.x.toFixed(2)}, ${base.y.toFixed(2)}, ${base.z.toFixed(2)}), ` +
        `ordinary-start=(${ordinaryWalkStart.x.toFixed(2)}, ${ordinaryWalkStart.y.toFixed(2)}, ${ordinaryWalkStart.z.toFixed(2)}), ` +
        `previous=(${previous.x.toFixed(2)}, ${previous.y.toFixed(2)}, ${previous.z.toFixed(2)}), ` +
        `ended=(${position.x.toFixed(2)}, ${position.y.toFixed(2)}, ${position.z.toFixed(2)}), ` +
        `support=${ordinaryWalkSupport}`,
    );

    // The first supported standing-height frame marks contact with the upper
    // deck after the bounded descent. Verify that pose has real production
    // support, then release input and require a stable ordinary recovery.
    const support = await game.raycast({
      start: [
        ordinaryWalkStart.x,
        ordinaryWalkStart.y,
        ordinaryWalkStart.z,
      ],
      end: [
        ordinaryWalkStart.x,
        ordinaryWalkStart.y - 3,
        ordinaryWalkStart.z,
      ],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      support.hit &&
        support.distance !== null &&
        support.distance > 0.5 &&
        support.distance < 1.25 &&
        support.hit_normal !== null &&
        support.hit_normal[1] > 0.5 &&
        support.collision_group !== "player",
      `the restored standing pose must have collision-valid support; ` +
        `ordinary-start=(${ordinaryWalkStart.x.toFixed(2)}, ${ordinaryWalkStart.y.toFixed(2)}, ${ordinaryWalkStart.z.toFixed(2)}), ` +
        `support=${JSON.stringify(support)}`,
    );
    await game.step({ frames: 120 });
    const landed = await game.player.position();
    await game.step({ frames: 60 });
    const stable = await game.player.position();
    const stableSupport = await game.raycast({
      start: [stable.x, stable.y, stable.z],
      end: [stable.x, stable.y - 3, stable.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      stable.x > ladder.x &&
        stable.z > ladder.z + 0.35 &&
        stable.y > ladderTop - 1.5 &&
        stable.y < ladderTop + 0.2 &&
        Math.abs(stable.y - landed.y) < 0.1 &&
        Math.hypot(
          stable.x - ordinaryWalkStart.x,
          stable.z - ordinaryWalkStart.z,
        ) < 1.0 &&
        stableSupport.hit &&
        stableSupport.distance !== null &&
        stableSupport.distance > 0.5 &&
        stableSupport.distance < 2.0 &&
        stableSupport.hit_normal !== null &&
        stableSupport.hit_normal[1] > 0.5 &&
        stableSupport.collision_group !== "player",
      `the completed continuous top-out must settle on the supported deck; ` +
        `ladder=(${ladder.x.toFixed(2)}, ${ladderTop.toFixed(2)}, ${ladder.z.toFixed(2)}), ` +
        `ordinary-start=(${ordinaryWalkStart.x.toFixed(2)}, ${ordinaryWalkStart.y.toFixed(2)}, ${ordinaryWalkStart.z.toFixed(2)}), ` +
        `landed=(${landed.x.toFixed(2)}, ${landed.y.toFixed(2)}, ${landed.z.toFixed(2)}), ` +
        `stable=(${stable.x.toFixed(2)}, ${stable.y.toFixed(2)}, ${stable.z.toFixed(2)}), ` +
        `stable-support=${JSON.stringify(stableSupport)}`,
    );

    // Prove the supported pose is the usable upper room. Reorient and walk
    // first along +Z past the ladder opening, then +X into the room using only
    // ordinary production locomotion.
    await game.input.lookAtWorldPoint([
      stable.x,
      stable.y + 1.6,
      ladder.z + 4,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let deckSide = stable;
    for (let elapsed = 0; elapsed < 60; elapsed += 2) {
      await game.step({ frames: 2 });
      deckSide = await game.player.position();
      if (deckSide.z > stable.z + 0.75) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });

    await game.input.lookAtWorldPoint([
      deckSide.x + 8,
      deckSide.y + 1.6,
      deckSide.z,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let advanced = deckSide;
    for (let elapsed = 0; elapsed < 120; elapsed += 5) {
      await game.step({ frames: 5 });
      advanced = await game.player.position();
      if (advanced.x > deckSide.x + 1.5) {
        break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 60 });
    const recovered = await game.player.position();
    const recoveredSupport = await game.raycast({
      start: [recovered.x, recovered.y, recovered.z],
      end: [recovered.x, recovered.y - 3, recovered.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      deckSide.z > stable.z + 0.5 &&
        recovered.x > ladder.x + 3 &&
        recovered.z > ladder.z + 1 &&
        recovered.x > deckSide.x + 1 &&
        recovered.y > ladderTop - 1.5 &&
        recovered.y < ladderTop + 0.2 &&
        recoveredSupport.hit &&
        recoveredSupport.distance !== null &&
        recoveredSupport.distance > 0.5 &&
        recoveredSupport.distance < 2.0 &&
        recoveredSupport.hit_normal !== null &&
        recoveredSupport.hit_normal[1] > 0.5 &&
        recoveredSupport.collision_group !== "player",
      `the continuous top-out must permit ordinary recovery into the upper room; ` +
        `stable=(${stable.x.toFixed(2)}, ${stable.y.toFixed(2)}, ${stable.z.toFixed(2)}), ` +
        `deck-side=(${deckSide.x.toFixed(2)}, ${deckSide.y.toFixed(2)}, ${deckSide.z.toFixed(2)}), ` +
        `recovered=(${recovered.x.toFixed(2)}, ${recovered.y.toFixed(2)}, ${recovered.z.toFixed(2)}), ` +
        `recovered-support=${JSON.stringify(recoveredSupport)}`,
    );
  },
);

// Issue #657's Eng1 blocker is a different Dark BreakClimb shape from Rick1:
// authored ladder 317 ends under a thick terrain slab. The player must rise
// through the slab's two locally sampled wall planes, then descend to the
// lower far-side deck. Treating the first plane as an ordinary full-height
// wall caps the standing capsule near y=-14.64 forever.
test(
  "flat climbing: eng1 ladder 317 crosses its local wall onto supported deck",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    // Runtime ids are unstable; object/template 317 and its authored name are
    // the durable mission identity. Derive both contact poses from that
    // authored object rather than baking its world coordinates into the test.
    const ladder = (
      await game.entities.list({ filter: "Ladder 16'", limit: 100 })
    ).entities.find((entity) => entity.template_id === 317);
    assert.ok(ladder, "eng1 should contain authored Ladder 16' object 317");
    const [ladderX, ladderY, ladderZ] = ladder.position;
    const standingY = ladderY + 1.243978;

    // Negative control: the north face is behind ordinary world terrain. A
    // production-forward push toward -Z must not phase through that wall or
    // gain the ladder's top-out rise.
    await game.player.teleport({
      x: ladderX,
      y: standingY,
      z: ladderZ + 0.99,
    });
    await game.input.lookAtWorldPoint([
      ladderX,
      standingY + 1.6,
      ladderZ - 10,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const wrongFace = await game.player.position();
    assert.ok(
      wrongFace.z > ladderZ + 0.5 && wrongFace.y < ladderY + 2.5,
      `the terrain-occluded face must remain a wall; ladder=(${ladderX.toFixed(2)}, ${ladderY.toFixed(2)}, ${ladderZ.toFixed(2)}), ` +
        `ended=(${wrongFace.x.toFixed(2)}, ${wrongFace.y.toFixed(2)}, ${wrongFace.z.toFixed(2)})`,
    );

    // Setup-only correct-face contact. No simulation frame occurs between the
    // relocation and production input, matching the campaign fixture while
    // keeping the test independent of a private save.
    await game.player.teleport({
      x: ladderX,
      y: standingY,
      z: ladderZ - 0.68944,
    });
    const start = await game.player.position();
    await game.input.lookAtWorldPoint([
      start.x,
      start.y + 1.6,
      start.z + 10,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    let crossedWall = false;
    let supportedLanding = false;
    let landing = start;
    let landingSupport: RayCastResult | null = null;
    for (let elapsed = 0; elapsed < 360; elapsed += 1) {
      await game.step({ frames: 1 });
      landing = await game.player.position();
      crossedWall ||= landing.y > ladderY + 3.5;
      if (landing.z > ladderZ + 0.7 && landing.y < ladderY + 2.5) {
        landingSupport = await game.raycast({
          start: [landing.x, landing.y, landing.z],
          end: [landing.x, landing.y - 3, landing.z],
          collision_groups: ["world", "entity", "selectable"],
          ignore_sensors: true,
        });
        supportedLanding =
          landingSupport.hit &&
          landingSupport.distance !== null &&
          landingSupport.distance > 0.5 &&
          landingSupport.distance < 2 &&
          landingSupport.hit_normal !== null &&
          landingSupport.hit_normal[1] > 0.5;
        if (supportedLanding) break;
      }
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);

    assert.ok(
      crossedWall && supportedLanding,
      `correct-face forward input must cross the local wall and reach supported far-side standing; ` +
        `start=(${start.x.toFixed(2)}, ${start.y.toFixed(2)}, ${start.z.toFixed(2)}), ` +
        `ended=(${landing.x.toFixed(2)}, ${landing.y.toFixed(2)}, ${landing.z.toFixed(2)}), ` +
        `support=${JSON.stringify(landingSupport)}`,
    );

    await game.step({ frames: 180 });
    const stable = await game.player.position();
    const stableSupport = await game.raycast({
      start: [stable.x, stable.y, stable.z],
      end: [stable.x, stable.y - 3, stable.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      stable.z > ladderZ + 0.7 &&
        Math.abs(stable.y - standingY) < 0.1 &&
        Math.hypot(stable.x - landing.x, stable.z - landing.z) < 0.25 &&
        stableSupport.hit &&
        stableSupport.distance !== null &&
        stableSupport.distance > 0.5 &&
        stableSupport.distance < 2 &&
        stableSupport.hit_normal !== null &&
        stableSupport.hit_normal[1] > 0.5,
      `release must remain stable on Eng1's far-side deck; ` +
        `landing=(${landing.x.toFixed(2)}, ${landing.y.toFixed(2)}, ${landing.z.toFixed(2)}), ` +
        `stable=(${stable.x.toFixed(2)}, ${stable.y.toFixed(2)}, ${stable.z.toFixed(2)}), ` +
        `support=${JSON.stringify(stableSupport)}`,
    );
  },
);

// Issue #657's Hydro2 extension is a third authored geometry family: eleven
// separate Rick Ladder rungs climb the north face of a narrow Sector C shaft,
// while the office landing sits beyond the terrain slab at the column top.
// Ordinary ascent reaches the underside but cannot expand or establish support.
test(
  "flat climbing: hydro2 Sector C rung stack reaches the upper office",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8109),
    });
    await game.step({ frames: 5 });

    // Runtime ids are unstable. Mission object 551 is the durable anchor for
    // this stack; discover the rest by exact authored name and shared column
    // geometry rather than baking its world coordinates into the scenario.
    const rungs = (
      await game.entities.list({ filter: "Rick Ladder", limit: 100 })
    ).entities;
    const anchor = rungs.find((entity) => entity.template_id === 551);
    assert.ok(anchor, "hydro2 should contain Sector C Rick Ladder object 551");
    const [ladderX, , ladderZ] = anchor.position;
    const column = rungs.filter(
      (entity) =>
        entity.name === "Rick Ladder" &&
        Math.hypot(
          entity.position[0] - ladderX,
          entity.position[2] - ladderZ,
        ) < 0.05,
    );
    const stableIds = new Set(column.map((entity) => entity.template_id));
    const ladderBottom = Math.min(...column.map((entity) => entity.position[1]));
    const ladderTop = Math.max(...column.map((entity) => entity.position[1]));
    assert.ok(
      column.length === 11 &&
        stableIds.has(551) &&
        stableIds.has(564) &&
        ladderTop - ladderBottom > 7.5,
      `expected the authored 11-rung Sector C column, got ids=${JSON.stringify([...stableIds].sort())}, ` +
        `span=${(ladderTop - ladderBottom).toFixed(2)}`,
    );

    // Geometry-relative setup reproduces the campaign save's supported
    // north-face contact. The climb and top-out use only production input.
    const standingFloorOffset = 1.243978;
    const northFaceOffset = 0.5712;
    await game.player.teleport({
      x: ladderX,
      y: ladderBottom + standingFloorOffset,
      z: ladderZ + northFaceOffset,
    });
    await game.step({ frames: 30 });
    const start = await game.player.position();
    const startSupport = await game.raycast({
      start: [start.x, start.y, start.z],
      end: [start.x, start.y - 3, start.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      Math.hypot(start.x - ladderX, start.z - (ladderZ + northFaceOffset)) < 0.05 &&
        startSupport.hit &&
        startSupport.distance !== null &&
        startSupport.distance > 0.5 &&
        startSupport.distance < 2 &&
        startSupport.hit_normal !== null &&
        startSupport.hit_normal[1] > 0.5,
      `expected supported north-face contact; start=${JSON.stringify(start)}, ` +
        `support=${JSON.stringify(startSupport)}`,
    );

    await game.input.lookAtWorldPoint([
      ladderX,
      ladderTop + 4,
      ladderZ - 10,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);

    let landing = start;
    let landingSupport: RayCastResult | null = null;
    let supportedLanding = false;
    for (let elapsed = 0; elapsed < 720; elapsed += 1) {
      await game.step({ frames: 1 });
      landing = await game.player.position();
      const crossedIntoOffice = landing.z < ladderZ - 0.4;
      if (!crossedIntoOffice) continue;
      landingSupport = await game.raycast({
        start: [landing.x, landing.y, landing.z],
        end: [landing.x, landing.y - 3, landing.z],
        collision_groups: ["world", "entity", "selectable"],
        ignore_sensors: true,
      });
      supportedLanding =
        landingSupport.hit &&
        landingSupport.distance !== null &&
        landingSupport.distance > 0.5 &&
        landingSupport.distance < 2 &&
        Math.abs(landingSupport.distance - standingFloorOffset) < 0.05 &&
        landingSupport.hit_point !== null &&
        landingSupport.hit_point[1] >= ladderTop - 0.1 &&
        landingSupport.hit_normal !== null &&
        landingSupport.hit_normal[1] > 0.5;
      if (supportedLanding) break;
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    assert.ok(
      supportedLanding,
      `continuous north-face input must cross the Sector C lip onto supported office floor; ` +
        `ladder=(${ladderX.toFixed(2)}, ${ladderTop.toFixed(2)}, ${ladderZ.toFixed(2)}), ` +
        `start=(${start.x.toFixed(2)}, ${start.y.toFixed(2)}, ${start.z.toFixed(2)}), ` +
        `ended=(${landing.x.toFixed(2)}, ${landing.y.toFixed(2)}, ${landing.z.toFixed(2)}), ` +
        `support=${JSON.stringify(landingSupport)}`,
    );

    await game.step({ frames: 180 });
    const stable = await game.player.position();
    const stableSupport = await game.raycast({
      start: [stable.x, stable.y, stable.z],
      end: [stable.x, stable.y - 3, stable.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      stable.z < ladderZ - 0.4 &&
        Math.abs(stable.y - landing.y) < 0.1 &&
        Math.hypot(stable.x - landing.x, stable.z - landing.z) < 0.25 &&
        stableSupport.hit &&
        stableSupport.distance !== null &&
        stableSupport.distance > 0.5 &&
        stableSupport.distance < 2 &&
        stableSupport.hit_point !== null &&
        stableSupport.hit_point[1] >= ladderTop - 0.1 &&
        stableSupport.hit_normal !== null &&
        stableSupport.hit_normal[1] > 0.5,
      `the office landing must remain stable after release; ` +
        `landing=${JSON.stringify(landing)}, stable=${JSON.stringify(stable)}, ` +
        `support=${JSON.stringify(stableSupport)}`,
    );

    // Reorient deeper into the office and prove the restored standing capsule
    // can use ordinary locomotion instead of merely perching on the lip.
    await game.input.lookAtWorldPoint([
      stable.x,
      stable.y + 1.6,
      stable.z - 6,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 60 });
    const office = await game.player.position();
    const officeSupport = await game.raycast({
      start: [office.x, office.y, office.z],
      end: [office.x, office.y - 3, office.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      stable.z - office.z > 0.75 &&
        officeSupport.hit &&
        officeSupport.distance !== null &&
        officeSupport.distance > 0.5 &&
        officeSupport.distance < 2 &&
        officeSupport.hit_point !== null &&
        officeSupport.hit_point[1] >= ladderTop - 0.1 &&
        officeSupport.hit_normal !== null &&
        officeSupport.hit_normal[1] > 0.5,
      `the supported top-out must permit ordinary onward movement into the office; ` +
        `stable=${JSON.stringify(stable)}, office=${JSON.stringify(office)}, ` +
        `support=${JSON.stringify(officeSupport)}`,
    );
  },
);
