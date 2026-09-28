import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { TrailState } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

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
    assert.deepEqual(await game.player.trail(), [], "the trail is off by default");

    await game.devParams.set("player_trail", 1);
    await teleportVerified(game, { x: -5.5, y: 1.5, z: 0 });
    await game.step({ frames: 30 });
    const start = await game.player.position();
    const eyeY = start.y + (await game.info()).player.camera_offset[1];
    await game.input.lookAtWorldPoint([-7, eyeY, 0]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 200 });
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const trail = await game.player.trail();
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
    assert.deepEqual(await game.player.trail(), [], "turning the trail off clears it");
  },
);
