import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
// Bare save name copied into DARK_ASSET_PATH/saves by the caller. Retail save
// payloads are copyrighted and intentionally remain outside the repository.
const ops4LiftSave = process.env.SHOCK2_OPS4_LIFT_SAVE;
const saveE2eEnabled = e2eEnabled && Boolean(ops4LiftSave);

// Stable mission identities (`cargo dq entities ops4.mis <id>`). Runtime ids
// are assigned afresh on every load and must be discovered each trial.
const LIFT = 572;
const LIFT_WALLS = 129;
const UPPER_BUTTON = 600;
const LOWER_BUTTON = 599;

function onlyEntity(entities: EntitySummary[], templateId: number): EntitySummary {
  assert.equal(
    entities.length,
    1,
    `expected one live entity for template ${templateId}, got ${JSON.stringify(entities)}`,
  );
  return entities[0];
}

async function entity(game: GameServer, templateId: number): Promise<EntitySummary> {
  return onlyEntity(await game.entities.byTemplate(templateId), templateId);
}

async function squeeze(game: GameServer): Promise<void> {
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
}

/**
 * Regression for #592: Ops4 mission object 129 authors SPHERE physics with an
 * explicit zero radius. The runtime must keep its crash-safe tiny body out of
 * character collision; otherwise a centered passenger reaches that invisible
 * point around ascent frame 80, is deflected sideways, loses lift support, and
 * falls back to the lower station.
 */
test(
  "Ops4 Lift 1 carries a centered passenger past dimensionless Lift 1 Walls",
  { skip: !saveE2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8489),
      echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
    });

    for (let trial = 1; trial <= 4; trial += 1) {
      await game.load(ops4LiftSave!);
      await game.step({ frames: 1 });

      const lift = await entity(game, LIFT);
      const walls = await entity(game, LIFT_WALLS);
      const upperButton = await entity(game, UPPER_BUTTON);
      const lowerButton = await entity(game, LOWER_BUTTON);

      const wallsBodies = await game.physics.bodies({ entityId: walls.id });
      assert.equal(wallsBodies.bodies.length, 1, "Lift 1 Walls should retain one body");
      assert.equal(
        wallsBodies.bodies[0].blocks_player,
        false,
        "zero-radius Lift 1 Walls must not become solid to the player",
      );
      assert.equal(
        wallsBodies.bodies[0].blocks_actor,
        false,
        "zero-radius Lift 1 Walls must not become solid to actors",
      );

      // Deterministically exercise the authored elevator/button scripts while
      // the player remains away from the shaft: send the lift down, then make
      // one empty up/down round trip. This preserves the clean point body that
      // earlier diagnostics accidentally knocked away before boarding.
      await game.entities.sendMessage(upperButton.id, { type: "Frob" });
      await game.step({ frames: 300 });
      await game.entities.sendMessage(lowerButton.id, { type: "Frob" });
      await game.step({ frames: 300 });
      await game.entities.sendMessage(lowerButton.id, { type: "Frob" });
      await game.step({ frames: 300 });

      const lowerLift = await entity(game, LIFT);
      assert.ok(
        Math.abs(lowerLift.position[1] + 15.8) < 0.01,
        `trial ${trial}: lift must be at the lower station, got ${lowerLift.position[1]}`,
      );

      // Match the clean campaign evidence exactly. The final activation is a
      // genuine surface aim + production squeeze, not a script-message bypass.
      await game.player.teleport({ x: 45.3, y: -14.356001, z: -106.0 });
      const aim = await game.player.aimAt(lowerButton, {
        hitbox: "surface",
        visibility: "required",
      });
      assert.equal(aim.target_confirmed, true, `trial ${trial}: lower button aim`);
      await game.step({ frames: 2 });
      await squeeze(game);

      let elapsed = 0;
      for (const target of [15, 30, 60, 90, 120, 300]) {
        await game.step({ frames: target - elapsed });
        elapsed = target;
      }

      const finalPlayer = await game.player.position();
      const finalLift = await entity(game, LIFT);
      const horizontalDrift = Math.hypot(
        finalPlayer.x - finalLift.position[0],
        finalPlayer.z - finalLift.position[2],
      );
      const supportOffset = finalPlayer.y - finalLift.position[1];

      assert.ok(
        finalLift.position[1] > -11.0,
        `trial ${trial}: lift should reach the upper station, got ${finalLift.position[1]}`,
      );
      assert.ok(
        horizontalDrift < 0.05,
        `trial ${trial}: passenger drifted off center by ${horizontalDrift}`,
      );
      assert.ok(
        Math.abs(supportOffset - 1.444) < 0.05,
        `trial ${trial}: passenger lost support (offset ${supportOffset})`,
      );
    }
  },
);
