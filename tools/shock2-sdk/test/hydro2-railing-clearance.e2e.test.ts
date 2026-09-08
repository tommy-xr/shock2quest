import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end regression for #808 against the two authentic Hydro2 Railing
// Terminators and ordinary production thumbstick locomotion. Opt in with:
//
//   npm run test:e2e
//
// Negative-first: on e616231, crouching reaches x=59.2 but the standing
// capsule stops at x=56.12 because the two rendered rail ends form a thin
// collider band across the continuous floor and authored AIPATH route.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "standing locomotion crosses Hydro2's spring-head railing band",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8288),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });
    await game.step({ frames: 2 });

    // Stable mission object IDs, never launch-local runtime entity IDs.
    const [southRail] = await game.entities.byTemplate(228);
    const [northRail] = await game.entities.byTemplate(1701);
    assert.ok(southRail && northRail, "both Railing Terminators must be instantiated");
    for (const rail of [southRail, northRail]) {
      assert.equal(rail.name, "Railing Terminator");
      const bodies = await game.physics.bodies({ entityId: rail.id });
      assert.equal(bodies.total_count, 1, `rail ${rail.template_id} must have its authored OBB`);
      assert.equal(bodies.bodies[0]?.blocks_player, true);
      assert.equal(bodies.bodies[0]?.is_sensor, false);
    }

    const stage = { x: 54.8, y: -0.796, z: 24.1 };
    const crossStanding = async () => {
      await teleportVerified(game, stage);
      await game.step({ frames: 30 });
      const start = await game.player.position();
      await game.input.lookAtWorldPoint([
        start.x + 10,
        start.y + PLAYER_EYE_HEIGHT_WORLD,
        start.z,
      ]);
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 90 });
      await game.input.set("right_hand.thumbstick", [0, 0]);
      return { start, end: await game.player.position() };
    };

    const standing = await crossStanding();
    assert.ok(
      standing.end.x > 58.4,
      `standing player must cross the band (${JSON.stringify(standing)})`,
    );
    assert.ok(
      Math.abs(standing.end.z - standing.start.z) < 0.25,
      `standing route must remain in the corridor (${JSON.stringify(standing)})`,
    );

    // Fixture control: the pre-fix workaround remains a legal route too.
    await teleportVerified(game, stage);
    await game.step({ frames: 30 });
    await game.input.set("crouch", 1);
    await game.step({ frames: 10 });
    const crouchedStart = await game.player.position();
    const crouchedEyeHeight = (await game.info()).player.camera_offset?.[1];
    await game.input.lookAtWorldPoint([
      crouchedStart.x + 10,
      crouchedStart.y + (crouchedEyeHeight ?? PLAYER_EYE_HEIGHT_WORLD),
      crouchedStart.z,
    ]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const crouchedEnd = await game.player.position();
    assert.ok(
      crouchedEnd.x > 58.4,
      `crouched player must still cross the band (${JSON.stringify({ crouchedStart, crouchedEnd })})`,
    );
  },
);
