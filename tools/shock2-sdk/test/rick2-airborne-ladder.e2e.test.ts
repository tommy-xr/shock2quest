import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { findRepoRoot, GameServer } from "../src/index.js";
import type { Position, RayCastResult, Vec3 } from "../src/index.js";

// Private 25th Anniversary campaign frontier immediately before the authored
// Pipe 46 -> Ladder 210 jump. The save remains local and is never committed.
const FIXTURE_SAVE = process.env.SHOCK2_RICK2_PIPE46_SAVE ?? "frontier";
const FIXTURE_SHA256 =
  "803238ed47de5b266da609f986c83d84f67dd63e04de751397f58f999a7441e6";
const LADDER = 210;

function savePath(saveName: string): string | undefined {
  const root = findRepoRoot(process.cwd()) ?? process.cwd();
  return [process.env.DARK_ASSET_PATH, join(root, "Data"), join(root, "..", "Data")]
    .filter((path): path is string => Boolean(path))
    .map((path) => join(path, "saves", `${saveName}.sav`))
    .find(existsSync);
}

function distance(a: Position, b: Position): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

const fixturePath = savePath(FIXTURE_SAVE);
const enabled = process.env.SHOCK2_E2E === "1" && Boolean(fixturePath);

test(
  "Rick2 Pipe 46 jump acquires Ladder 210 while descending and reaches the main tunnel",
  { skip: !enabled, timeout: 600_000 },
  async (t) => {
    assert.ok(fixturePath);
    assert.equal(
      createHash("sha256").update(readFileSync(fixturePath)).digest("hex"),
      FIXTURE_SHA256,
      "the regression must use the accepted Pipe 46 campaign frontier",
    );

    await using game = await GameServer.launch({
      mission: "rick2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8301),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    const loaded = await game.load(FIXTURE_SAVE);
    assert.equal(loaded.success, true);
    assert.equal(loaded.mission.toLowerCase(), "rick2.mis");

    await game.input.set("crouch", 1);
    await game.input.setJump(false);
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 2 });
    const start = await game.player.position();
    assert.equal((await game.info()).player.hit_points, 24);

    const ladder = (await game.entities.byTemplate(LADDER))[0];
    assert.ok(ladder, "Rick2 must contain mission Ladder 210");
    const [ladderX, ladderY, ladderZ] = ladder.position;

    // Ordinary east movement reaches the campaign-proven takeoff point on
    // Pipe 46. No relocation or collider filtering is used.
    await game.input.lookAtWorldPoint([start.x + 10, start.y, start.z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let stage = start;
    for (let frame = 0; frame < 12 && stage.x < 166.4; frame += 1) {
      await game.step({ frames: 1 });
      stage = await game.player.position();
    }
    assert.ok(
      stage.x >= 166.35 && stage.x <= 166.55 && Math.abs(stage.z - start.z) < 0.08,
      `ordinary Pipe 46 stage must match the accepted takeoff: ${JSON.stringify({ start, stage })}`,
    );

    // Jump toward the pipe opening (horizontal dx/dz ratio 2.204), then at
    // the authored clearance frame turn due north and keep the original
    // forward input. The descending capsule remains within Ladder 210's real
    // south-face reach for many frames; it must hand off to controlled climb.
    const initialDx = 0.9107;
    const initialDz = 0.4132;
    const horizontal = 10;
    const rise = Math.tan((20 * Math.PI) / 180) * horizontal;
    await game.input.lookAtWorldPoint([
      stage.x + initialDx * horizontal,
      stage.y + rise,
      stage.z + initialDz * horizontal,
    ]);
    await game.input.setJump(true);
    await game.step({ frames: 1 });
    await game.input.setJump(false);
    await game.step({ frames: 7 });
    await game.input.lookAtWorldPoint([stage.x, stage.y + rise, stage.z + horizontal]);

    let previous = await game.player.position();
    let grip: Position | null = null;
    let risingFrames = 0;
    const trace: Position[] = [previous];
    for (let frame = 0; frame < 90; frame += 1) {
      await game.step({ frames: 1 });
      const position = await game.player.position();
      trace.push(position);
      const dy = position.y - previous.y;
      const nearSouthFace =
        Math.abs(position.x - ladderX) < 0.8 &&
        position.z < ladderZ &&
        ladderZ - position.z < 0.9;
      risingFrames = nearSouthFace && dy > 0.01 ? risingFrames + 1 : 0;
      if (risingFrames >= 2) {
        grip = position;
        break;
      }
      previous = position;
    }
    assert.ok(
      grip && grip.y >= 92.5,
      `the descending jump must acquire Ladder 210 before falling below y=92.5; ` +
        `ladder=${JSON.stringify(ladder.position)}, tail=${JSON.stringify(trace.slice(-12))}`,
    );

    // Keep the same ordinary north input through the ladder's normal climb and
    // top-out. Require a real upward-support hit and a short ordinary transfer
    // toward the connected main tunnel, never an unsupported intermediate.
    let supported: { position: Position; support: RayCastResult } | null = null;
    for (let frame = 0; frame < 420; frame += 1) {
      await game.step({ frames: 1 });
      const position = await game.player.position();
      if (position.y < ladderY + 2) continue;
      const support = await game.raycast({
        start: [position.x, position.y + 0.1, position.z],
        end: [position.x, position.y - 3, position.z],
        collision_groups: ["world", "entity", "selectable"],
        ignore_sensors: true,
      });
      if (
        support.hit_normal?.[1] !== undefined &&
        support.hit_normal[1] > 0.5 &&
        support.distance !== null &&
        support.distance > 0.4 &&
        support.distance < 2
      ) {
        supported = { position, support };
        break;
      }
    }
    assert.ok(
      supported,
      `controlled climb must reach authored support above Ladder 210; grip=${JSON.stringify(grip)}`,
    );

    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });
    const landed = await game.player.position();
    const eastTarget = [Math.max(169.6, landed.x + 2), landed.y, landed.z] as Vec3;
    await game.input.lookAtWorldPoint(eastTarget);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    let tunnel = landed;
    for (let frame = 0; frame < 120 && tunnel.x < 169.4; frame += 1) {
      await game.step({ frames: 1 });
      tunnel = await game.player.position();
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 30 });
    const settled = await game.player.position();
    await game.step({ frames: 60 });
    const stable = await game.player.position();
    // The crouched capsule's center is just east of the WorldRep floor edge;
    // sample beneath its west footprint where the same resolved capsule is
    // supported, rather than point-testing the unsupported center projection.
    const tunnelSupport = await game.raycast({
      start: [stable.x - 0.4, stable.y + 2, stable.z],
      end: [stable.x - 0.4, stable.y - 5, stable.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      stable.x >= 169.4 &&
        tunnelSupport.hit_point !== null &&
        Math.abs(tunnelSupport.hit_point[1] - 94.8) < 0.05 &&
        tunnelSupport.hit_normal !== null &&
        tunnelSupport.hit_normal[1] > 0.5 &&
        distance(stable, settled) < 0.05,
      `ordinary transfer must end stable on the main tunnel: ${JSON.stringify({ landed, tunnel, settled, stable, tunnelSupport })}`,
    );
    assert.equal((await game.info()).player.hit_points, 24);
    t.diagnostic(
      `Pipe46 ${JSON.stringify(start)} -> stage ${JSON.stringify(stage)} -> grip ${JSON.stringify(grip)} -> ` +
        `support ${JSON.stringify(supported)} -> tunnel ${JSON.stringify(stable)}`,
    );
  },
);
