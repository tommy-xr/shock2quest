import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Position } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const reproSave = process.env.SHOCK2_PORTAL_SEAM_SAVE;

async function walkTo(
  game: GameServer,
  target: [number, number],
  label: string,
  tolerance = 0.45,
): Promise<Position> {
  let position = await game.player.position();
  let best = Math.hypot(position.x - target[0], position.z - target[1]);
  let stalledFrames = 0;
  await game.input.set("right_hand.thumbstick", [0, 1]);
  for (let frame = 0; frame < 240; frame += 1) {
    if (frame % 8 === 0) {
      await game.input.lookAtWorldPoint([
        target[0],
        position.y + 0.48,
        target[1],
      ]);
    }
    await game.step({ frames: 1 });
    position = await game.player.position();
    const distance = Math.hypot(
      position.x - target[0],
      position.z - target[1],
    );
    if (distance <= tolerance) break;
    if (distance < best - 0.03) {
      best = distance;
      stalledFrames = 0;
    } else {
      stalledFrames += 1;
    }
    assert.ok(
      stalledFrames < 45,
      `${label} stalled at ${JSON.stringify(position)} (${distance.toFixed(3)} from target)`,
    );
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.ok(
    Math.hypot(position.x - target[0], position.z - target[1]) <= tolerance,
    `${label} did not reach ${JSON.stringify(target)}: ${JSON.stringify(position)}`,
  );
  return position;
}

// Rick1's high-deck return route is a shipped SMALL_CREATURE AIPATH passage
// between a WorldRep wall and the end of a live Pipe 24x3. The 1.6-foot
// crouched body fits the 1.72-foot opening, but the old 0.1-foot controller
// margin was charged on both sides and pinned the capsule at x=44.88 forever.
//
// The save is private campaign state and is supplied only to local/opt-in
// runs; it is never checked in or uploaded. From that authentic pose the test
// uses ordinary input only: cross cells 270 -> 269 -> 271, physically enter
// tripwire1259, and walk through its opened door1257.
test(
  "rick1: crouched player crosses the high-deck portal and opens door 1257",
  {
    skip: !e2eEnabled || !reproSave || !existsSync(reproSave),
    timeout: 600_000,
  },
  async () => {
    assert.ok(reproSave, "SHOCK2_PORTAL_SEAM_SAVE must name the private repro save");
    await using game = await GameServer.launch({
      mission: "rick1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8270),
      debugFlags: ["--save-file", reproSave],
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    const [tripwire] = await game.entities.byTemplate(1259);
    const [door] = await game.entities.byTemplate(1257);
    assert.ok(tripwire, "rick1 should contain authored tripwire1259");
    assert.ok(door, "rick1 should contain authored door1257");
    const closedDoor = await game.entities.detail(door.id);

    await game.input.set("crouch", 1);
    await game.input.set("jump", 0);
    await game.step({ frames: 20 });
    const start = await game.player.position();
    assert.ok(
      start.x > 44.7 && start.x < 45.1 && start.z > -15.8 && start.z < -15.3,
      `the repro must start at the blocked portal: ${JSON.stringify(start)}`,
    );

    const crossed = await walkTo(game, [42.4, -16.0], "portal crossing");
    assert.ok(
      crossed.x < 44.4,
      `ordinary crouched input must cross west of the portal, ended ${JSON.stringify(crossed)}`,
    );
    await game.step({ frames: 60 });
    const stable = await game.player.position();
    const stablePlayer = (await game.info()).player;
    const support = await game.raycast({
      start: [stable.x, stable.y + 0.1, stable.z],
      end: [stable.x, stable.y - 2, stable.z],
      collision_groups: ["world", "entity", "selectable"],
      ignore_sensors: true,
    });
    assert.ok(
      Math.abs(stable.y - 62.184) < 0.08 &&
        Math.abs(stablePlayer.camera_offset[1] - 0.48) < 0.02 &&
        support.hit &&
        support.hit_point !== null &&
        Math.abs(support.hit_point[1] - 61.6) < 0.05 &&
        support.hit_normal !== null &&
        support.hit_normal[1] > 0.5,
      `the crossed player must remain crouched and supported on y61.6: ` +
        `stable=${JSON.stringify(stable)}, camera=${JSON.stringify(stablePlayer.camera_offset)}, ` +
        `support=${JSON.stringify(support)}`,
    );

    await walkTo(game, [37.6, -16.0], "cell269 corridor");
    await walkTo(
      game,
      [tripwire.position[0], tripwire.position[2]],
      "physical tripwire1259 entry",
      0.35,
    );
    await game.step({ frames: 120 });
    const openedDoor = await game.entities.detail(door.id);
    const doorTravel = Math.hypot(
      openedDoor.position[0] - closedDoor.position[0],
      openedDoor.position[1] - closedDoor.position[1],
      openedDoor.position[2] - closedDoor.position[2],
    );
    assert.ok(
      doorTravel > 2,
      `physical tripwire1259 entry must open door1257: ` +
        `${JSON.stringify(closedDoor.position)} -> ${JSON.stringify(openedDoor.position)}`,
    );

    const beyondDoor = await walkTo(
      game,
      [tripwire.position[0], door.position[2] - 1.5],
      "opened door1257 crossing",
      0.55,
    );
    assert.ok(
      beyondDoor.z < door.position[2] - 0.8,
      `ordinary movement must continue through opened door1257: ${JSON.stringify(beyondDoor)}`,
    );
  },
);
