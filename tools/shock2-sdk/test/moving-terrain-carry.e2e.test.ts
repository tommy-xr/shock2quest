import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

// End-to-end coverage for support-motion transfer: a passenger standing on
// authored moving terrain holds their spot on it, for the whole ride.
//
// `command-tram.e2e.test.ts` covers the authored PhysAttach assembly moving as
// one body over a one-second window. This covers the other half - how exactly
// the *player* is carried - over a full transit, where a small per-second leak
// compounds into being left behind.
//
// Negative-first: with the character controller's own contact-based transfer,
// the passenger slid ~0.13 wu per second backwards along the deck and finished
// the command1 leg 0.75 wu behind where they boarded (the whole grav-lift pad
// is only 2.4 wu across, so the same leak strands a rider outright on a longer
// or faster ride).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRAM_BUTTON_OBJECT = 199;
const LIFT_OBJECT = 431;
const LIFT_LOWER_BUTTON_OBJECT = 469;
const LIFT_UPPER_BUTTON_OBJECT = 494;

/** Elevator Path node 101 - the far end of the tram's first leg. */
const STATION_TWO_X = -184.43343;

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

test(
  "command1.mis: a tram passenger rides the whole leg without sliding on the deck",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8188),
    });
    await game.step({ frames: 5 });

    const tram = only(
      (await game.entities.list({ filter: "Tram", limit: 30 })).entities.filter(
        (entity) => entity.name === "Tram",
      ),
      "authored Tram root",
    );
    const button = only(
      await game.entities.byTemplate(TRAM_BUTTON_OBJECT),
      "in-car tram button mission object",
    );

    // Teleport is setup only; board through the open side doorway with the
    // bounded, shape-cast-validated move.
    await game.player.teleport({ x: -377.6, y: -16.4, z: 5.2 });
    await game.step({ frames: 2 });
    const boarding = await game.player.moveTo({
      x: tram.position[0] - 0.25,
      y: -16.4,
      z: tram.position[2] + 0.08,
    });
    assert.equal(boarding.moved, true, "player should walk through the tram doorway");
    await game.step({ frames: 10 });

    const playerBefore = await game.player.position();
    const tramBefore = (await game.entities.detail(tram.id)).position;

    // The production squeeze edge on the in-car call button.
    const aim = await game.player.aimAt(button, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.entity_id, button.id, JSON.stringify(aim));
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);

    // The leg is 193 wu at the authored 12 wu/s; run past its arrival and let
    // the car settle at the far node.
    await game.step({ frames: 1140 });

    const playerAfter = await game.player.position();
    const tramAfter = (await game.entities.detail(tram.id)).position;

    assert.ok(
      Math.abs(tramAfter[0] - STATION_TWO_X) < 0.5,
      `the tram should have arrived at station 2 (x=${STATION_TWO_X}), got ${tramAfter[0]}`,
    );
    assert.ok(
      Math.abs(playerAfter.x - tramAfter[0]) < 2,
      `the passenger should arrive aboard the car: player=${playerAfter.x}, tram=${tramAfter[0]}`,
    );

    const slide =
      playerAfter.x - tramAfter[0] - (playerBefore.x - tramBefore[0]);
    assert.ok(
      Math.abs(slide) < 0.25,
      `the passenger must not slide along the deck during transit: slid ${slide.toFixed(3)} wu ` +
        `(boarded at ${(playerBefore.x - tramBefore[0]).toFixed(3)} from the car origin, ` +
        `arrived at ${(playerAfter.x - tramAfter[0]).toFixed(3)})`,
    );
  },
);

test(
  "command1.mis: the grav lift carries a rider up and back down in lockstep",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8189),
    });
    await game.step({ frames: 5 });

    const lift = only(await game.entities.byTemplate(LIFT_OBJECT), "grav Lift 1");
    const lowerButton = only(
      await game.entities.byTemplate(LIFT_LOWER_BUTTON_OBJECT),
      "lower lift call button",
    );
    const upperButton = only(
      await game.entities.byTemplate(LIFT_UPPER_BUTTON_OBJECT),
      "upper lift call button",
    );

    const pad = lift.position;
    await game.player.teleport({ x: pad[0], y: pad[1] + 1.2, z: pad[2] });
    await game.step({ frames: 60 });

    const standing = await game.player.position();
    const padY = (await game.entities.detail(lift.id)).position[1];
    const restingOffset = standing.y - padY;
    assert.ok(
      restingOffset > 0.5 && restingOffset < 2,
      `the player should be standing on the pad, not inside or above it (offset ${restingOffset})`,
    );

    // Vertical carry is the one that already worked (a rising deck simply
    // pushes the capsule up), so this is the regression guard for it.
    await game.entities.sendMessage(lowerButton.id, { type: "Frob" });
    await game.step({ frames: 180 });
    const raised = await game.player.position();
    const raisedPad = (await game.entities.detail(lift.id)).position[1];
    assert.ok(
      raised.y - standing.y > 3,
      `the lift should carry the player up its 3.65 wu shaft, rose ${raised.y - standing.y}`,
    );
    assert.ok(
      Math.abs(raised.y - raisedPad - restingOffset) < 0.1,
      `the player should still be standing on the pad at the top (offset ${raised.y - raisedPad} vs ${restingOffset})`,
    );

    await game.entities.sendMessage(upperButton.id, { type: "Frob" });
    await game.step({ frames: 180 });
    const lowered = await game.player.position();
    const loweredPad = (await game.entities.detail(lift.id)).position[1];
    assert.ok(
      Math.abs(lowered.y - standing.y) < 0.1,
      `the lift should return the player to where they boarded: ${lowered.y} vs ${standing.y}`,
    );
    assert.ok(
      Math.abs(lowered.y - loweredPad - restingOffset) < 0.1,
      `the player should still be standing on the pad at the bottom (offset ${lowered.y - loweredPad} vs ${restingOffset})`,
    );
  },
);
