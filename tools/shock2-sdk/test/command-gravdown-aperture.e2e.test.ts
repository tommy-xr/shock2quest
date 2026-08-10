import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Fresh-mission production regression for command1's GravDown103 south exit.
// Stable mission coordinates identify authored world geometry only; runtime
// entity ids remain launch-specific and are never hardcoded.
test(
  "command1: crouched GravDown movement exits the exact-height south aperture",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8524),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 10 });

    const hpBefore = (await game.info()).player.hit_points;
    const overlordBefore = await game.quests.get("overlord");

    // Enter crouch in open space first, then enter the authored GravDown room
    // sensor so this setup exercises the real ROOM_DB -> CoreRoom gravity
    // chain. The lower slot lies just past that sensor: staging the exact
    // reviewed pose also reproduces its ordinary SensorEndIntersect reset to
    // downward world gravity before the crossing begins.
    await game.input.set("crouch", 1);
    await game.step({ frames: 2 });
    assert.ok(
      (await game.player.teleport({ x: -310.4, y: 1.4, z: 87.59998 }))
        .success,
      "the player must enter the authored GravDown room sensor",
    );
    await game.step({ frames: 2 });
    assert.ok(
      (await game.player.teleport({ x: -311.8, y: -2.49647, z: 87.59998 }))
        .success,
      "the crouched capsule must fit in the GravDown chute",
    );
    await game.step({ frames: 2 });
    assert.ok(
      (await game.player.teleport({ x: -311.8, y: -2.49647, z: 87.59998 }))
        .success,
      "the exact reviewed pre-crossing pose must remain collision-valid",
    );
    const start = await game.player.position();

    // Aim the production camera horizontally toward world -Z/south while
    // accounting for command1's authored pawn rotation. This is ordinary flat
    // locomotion with the real crouch held throughout: no validated-move
    // helper, jump, teleport, entity mutation, or direct gravity/quest message
    // performs the crossing.
    const cameraOffset = (await game.info()).player.camera_offset?.[1] ?? 0.48;
    await game.input.lookAtWorldPoint([
      start.x,
      start.y + cameraOffset,
      start.z - 10,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 18 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    await game.step({ frames: 120 });

    const landed = await game.player.position();
    assert.ok(
      landed.z < 86.4 && Math.abs(landed.y - -2.596) < 0.15,
      `ordinary crouched movement must clear the aperture and settle on the ` +
        `authored low-corridor floor (start=${JSON.stringify(start)}, landed=${JSON.stringify(landed)})`,
    );
    const support = await game.raycast({
      start: [landed.x, landed.y, landed.z],
      end: [landed.x, landed.y - 2.0, landed.z],
      collision_groups: ["world"],
      ignore_sensors: true,
    });
    assert.ok(
      support.hit_point !== null &&
        support.hit_normal !== null &&
        Math.abs(support.hit_point[1] - -3.2) < 0.05 &&
        support.hit_normal[1] > 0.9,
      `the endpoint must have durable authored world support at y=-3.2: ${JSON.stringify(support)}`,
    );
    assert.equal((await game.info()).player.hit_points, hpBefore, "the exit must not damage the player");
    assert.equal(
      await game.quests.get("overlord"),
      overlordBefore,
      "the focused crossing must not enter the later tram/umbilical tripwire",
    );
  },
);
