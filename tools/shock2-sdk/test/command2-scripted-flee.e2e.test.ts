import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Quat, Vec3 } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// command2's `lovebegin` tripwire starts Siddons and Suarez running to
// `lovend` through a ramped corridor; a 30 s failsafe removes them after.
const TRIGGER_X = -51.1;
const SAMPLE_SECONDS = 27;
const ARRIVE_DISTANCE = 1.5;
// A fast turn that reverses on the very next tick is a heading zig-zag, not
// a turn. Wall-whisker flip-flopping produced it on ~43% of running ticks.
const ZIGZAG_RATE_DEG_S = 300;
const MAX_ZIGZAG_SHARE = 0.15;

function yawDeg([x, y, z, w]: Quat): number {
  return (Math.atan2(2 * (w * y + x * z), 1 - 2 * (y * y + x * x)) * 180) / Math.PI;
}

function xzDistance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[2] - b[2]);
}

async function entityId(game: GameServer, name: string): Promise<number> {
  const { entities } = await game.entities.list({ filter: name });
  const entity = entities.find((e) => e.name === name);
  assert.ok(entity, `command2 should contain ${name}`);
  return entity.id;
}

test(
  "command2's scripted flee routes Siddons and Suarez to their end marker without zig-zagging",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "command2.mis" });
    await game.step({ frames: 5 });

    const actors = { Siddons: await entityId(game, "Siddons"), Suarez: await entityId(game, "Suarez") };
    const lovend = (await game.entities.detail(await entityId(game, "lovend"))).position;

    // Walk into the tripwire, as the player does.
    await game.player.teleport({ x: -53.5, y: -14.4, z: 0.0 });
    await game.input.set("head.look", [180, 0]);
    await game.step({ frames: 10 });
    await game.input.set("right_hand.thumbstick", [0, 1]);
    for (let frame = 0; (await game.info()).player.position[0] <= TRIGGER_X; frame++) {
      assert.ok(frame < 600, "the player should reach the lovebegin tripwire");
      await game.step({ frames: 1 });
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);

    const tracks: Record<string, { position: Vec3; yaw: number }[]> = { Siddons: [], Suarez: [] };
    for (let tick = 0; tick < SAMPLE_SECONDS * 60; tick++) {
      await game.step({ frames: 1 });
      for (const [name, id] of Object.entries(actors)) {
        const state = await game.entities.animation(id);
        if (state) tracks[name].push({ position: state.position, yaw: yawDeg(state.rotation) });
      }
    }

    for (const [name, track] of Object.entries(tracks)) {
      const last = track[track.length - 1];
      assert.ok(last, `${name} should still exist before the failsafe`);
      assert.ok(
        xzDistance(last.position, lovend) < ARRIVE_DISTANCE,
        `${name} should reach lovend, ended ${xzDistance(last.position, lovend).toFixed(2)} away`,
      );

      // Yaw rate per tick while running, then count back-to-back reversals.
      const rates: (number | null)[] = [];
      for (let i = 1; i < track.length; i++) {
        const running = xzDistance(track[i].position, track[i - 1].position) * 60 > 0.5;
        const turn = ((track[i].yaw - track[i - 1].yaw + 540) % 360) - 180;
        rates.push(running ? turn * 60 : null);
      }
      const running = rates.filter((rate) => rate !== null).length;
      let zigzags = 0;
      for (let i = 1; i < rates.length; i++) {
        const [a, b] = [rates[i - 1], rates[i]];
        if (a !== null && b !== null && Math.abs(a) >= ZIGZAG_RATE_DEG_S && Math.abs(b) >= ZIGZAG_RATE_DEG_S && a * b < 0) {
          zigzags++;
        }
      }
      assert.ok(running > 300, `${name} should run for a while, ran ${running} ticks`);
      assert.ok(
        zigzags / running < MAX_ZIGZAG_SHARE,
        `${name} zig-zagged on ${((100 * zigzags) / running).toFixed(1)}% of ${running} running ticks`,
      );
    }
  },
);
