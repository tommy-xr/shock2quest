import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, otherHand, vrClimbLadder, vrTopOut } from "../src/index.js";
import type { Hand, TrailEvent, TrailState, Vec3 } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import { shootTrail } from "./helpers/trail.js";

// The player trail (`player_trail` dev param, GET /v1/player/trail) records
// one tagged sample per simulated frame. Climbing debug_ladder's ledge station
// must show the walk-up, the climb and the top-out in order.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "player trail: a ledge climb is recorded as supported, climbing, then top-out",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 5 });
    assert.deepEqual(
      await game.player.trail(),
      { samples: [], events: [] },
      "the trail is off by default",
    );

    await game.devParams.set("player_trail", 1);
    await teleportVerified(game, { x: -5.5, y: 1.5, z: 0 });
    await game.step({ frames: 30 });
    const start = await game.player.position();
    const eyeY = start.y + (await game.info()).player.camera_offset[1];
    await game.input.lookAtWorldPoint([-7, eyeY, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 200 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const { samples: trail, events } = await game.player.trail();
    assert.deepEqual(events, [], "flat records no hand events");
    assert.ok(
      trail.every((s) => s.hands === undefined),
      "flat records no hand paths",
    );
    assert.ok(trail.length > 200, `one sample per frame (got ${trail.length})`);
    const frames = trail.map((s) => s.frame);
    assert.deepEqual(frames, [...frames].sort((a, b) => a - b), "frames are in order");
    const order = trail.map((s) => s.state).filter((s, i, all) => s !== all[i - 1]);
    const first = (state: TrailState) => order.indexOf(state);
    // (The teleport drops the body a little first, so it may open airborne.)
    assert.ok(first("supported") >= 0, `stands on the floor: ${order.join(" > ")}`);
    assert.ok(first("climbing") > first("supported"), `then climbs: ${order.join(" > ")}`);
    assert.ok(first("top_out") > first("climbing"), `then tops out: ${order.join(" > ")}`);
    const climbing = trail.filter((s) => s.state === "climbing");
    assert.ok(
      Math.max(...climbing.map((s) => s.pos[1])) - start.y > 3,
      "the climbing samples rise up the ladder",
    );

    await game.devParams.set("player_trail", 0);
    await game.step({ frames: 1 });
    assert.deepEqual(
      await game.player.trail(),
      { samples: [], events: [] },
      "turning the trail off clears it",
    );
  },
);

// VR: the ledge station's ladder, hand over hand, then a top-out by a lip or a
// mid-deck grab (see vr-vault.e2e.test.ts for the stations' geometry). The
// trail must carry both hand paths, each hand's grips and releases in turn,
// and a top-out marker on the hold that was pulled over.
const LEDGE_TOP = 6.0;
for (const [name, depth] of [["lip", -7.05], ["mid-deck", -7.3]] as const) {
  test(
    `player trail (VR): a hand-over-hand ledge climb records both hands and a ${name} top-out`,
    { skip: !e2eEnabled, timeout: 600_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
      await game.step({ frames: 5 });
      await game.devParams.set("player_trail_seconds", 120);
      await game.devParams.set("player_trail", 1);
      await teleportVerified(game, { x: -6.55, y: 1.5, z: 0 });
      await game.step({ frames: 30 });
      const { anchor } = await vrClimbLadder(game, { near: [-6.8, 2.6, 0], untilY: LEDGE_TOP - 0.9 });
      const hold: Vec3 = [depth, LEDGE_TOP + 0.05, 0];
      const vaultHand = otherHand(anchor);
      await vrTopOut(game, vaultHand, hold);
      await shootTrail(game, `vr-ledge-${name}`);

      const { samples, events } = await game.player.trail();
      const withHands = samples.filter((s) => s.hands !== undefined);
      assert.equal(withHands.length, samples.length, "every VR sample has both hands");
      for (const hand of ["left", "right"] as const) {
        const ys = withHands.map((s) => s.hands![hand][1]);
        assert.ok(Math.max(...ys) - Math.min(...ys) > 3, `the ${hand} hand's path climbs the ladder`);
      }

      const topOuts = events.filter((e) => e.kind === "top_out");
      assert.equal(topOuts.length, 1, `one top-out: ${JSON.stringify(topOuts)}`);
      const [topOut] = topOuts;
      assert.equal(topOut.hand, vaultHand);
      assert.equal(topOut.hold, "ledge");
      assert.ok(
        Math.abs(topOut.pos[0] - depth) < 0.1 && Math.abs(topOut.pos[1] - LEDGE_TOP) < 0.1,
        `the top-out marks the ${name} hold ${JSON.stringify(hold)}, got ${JSON.stringify(topOut.pos)}`,
      );

      // Per hand: grip, release, grip, ... and each hand climbs on the
      // ladder more than once. Every release before the vault is a handoff:
      // the other hand already holds.
      const holding: Record<Hand, boolean> = { left: false, right: false };
      for (const event of events.filter((e) => e.kind !== "top_out")) {
        const expected: TrailEvent["kind"] = holding[event.hand] ? "release" : "grip";
        assert.equal(event.kind, expected, `${event.hand} at frame ${event.frame}: ${summary(events)}`);
        if (event.kind === "release" && event.frame < topOut.frame) {
          assert.ok(holding[otherHand(event.hand)], `a handoff at frame ${event.frame}: ${summary(events)}`);
        }
        holding[event.hand] = event.kind === "grip";
      }
      assert.deepEqual(holding, { left: false, right: false }, "the vault let go of everything");
      for (const hand of ["left", "right"] as const) {
        const ladderGrips = events.filter((e) => e.hand === hand && e.kind === "grip" && e.hold === "ladder");
        assert.ok(ladderGrips.length >= 2, `${hand} gripped the ladder hand over hand: ${summary(events)}`);
      }
      const releasedAtVault = events.filter((e) => e.kind === "release" && e.frame === topOut.frame);
      assert.ok(
        releasedAtVault.some((e) => e.hand === vaultHand && e.hold === "ledge"),
        `the vault opens the ledge hand: ${summary(events)}`,
      );
    },
  );
}

const summary = (events: TrailEvent[]) =>
  events.map((e) => `${e.frame}:${e.hand[0]}-${e.kind}(${e.hold})`).join(" ");
