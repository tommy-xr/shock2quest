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
const OUTER_DOORS = [547, 192] as const;
const INNER_DOORS = [193, 194] as const;

type Point = { x: number; y: number; z: number };

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

async function frob(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<void> {
  const target = await entity(game, templateId);
  const aim = await game.player
    .aimAt(target, {
      hitbox: "surface",
      visibility: "required",
    })
    .catch(async (error: unknown) => {
      const position = await game.player.position();
      throw new Error(`${label} from ${JSON.stringify(position)}: ${String(error)}`, {
        cause: error,
      });
    });
  assert.equal(aim.target_confirmed, true, `${label}: surface aim`);
  assert.equal(aim.entity_id, target.id, `${label}: aimed runtime entity`);
  await game.step({ frames: 2 });
  await squeeze(game);
}

async function moveAndSettle(
  game: GameServer,
  target: Point,
  label: string,
  allowBlocked = false,
) {
  const result = await game.player.moveTo(target);
  // Match the campaign playtest primitive exactly: every bounded movement is
  // followed by eight ordinary production frames before the next action.
  await game.step({ frames: 8 });
  if (!allowBlocked) {
    assert.equal(result.blocked, false, `${label}: ${JSON.stringify(result)}`);
    const position = await game.player.position();
    assert.ok(
      Math.hypot(position.x - target.x, position.z - target.z) < 0.1,
      `${label}: did not reach waypoint: ${JSON.stringify({ target, result, position })}`,
    );
  }
  return result;
}

async function traverseToUpperLift(game: GameServer, trial: number): Promise<void> {
  for (const door of OUTER_DOORS) {
    await frob(game, door, `trial ${trial}: outer door ${door}`);
  }
  await game.step({ frames: 25 });

  // The preserved campaign save contains a Red Assassin corpse in this
  // doorway. Crouched bounded locomotion walks around its edge; the midpoint
  // may report blocked after making useful progress, exactly as in playtest.
  await game.input.set("crouch", 1);
  await game.step({ frames: 3 });
  await moveAndSettle(game, { x: 65, y: -9.556, z: -97.8 }, "outer threshold");
  await moveAndSettle(
    game,
    { x: 65, y: -9.556, z: -99.5 },
    "corpse edge",
    true,
  );
  await moveAndSettle(game, { x: 65, y: -9.556, z: -101 }, "past corpse");
  await moveAndSettle(game, { x: 62, y: -9.556, z: -102.2 }, "inner doors");
  await game.input.set("crouch", 0);
  await game.step({ frames: 3 });

  // Crossing the authored New Tripwire at the inner threshold opens both
  // linked panels. Let that real sensor/door path finish before walking on;
  // directly frobbing the already-moving panels is timing-dependent because
  // their selectable centers slide behind the doorway brush.
  for (const door of INNER_DOORS) await entity(game, door);
  await game.step({ frames: 25 });
  let through = await moveAndSettle(
    game,
    { x: 60.7, y: -9.556, z: -102.4 },
    "inner threshold",
    true,
  );
  if (through.blocked) {
    // A final bounded retry covers arriving during the last closing fraction;
    // setup still uses only the authored tripwire and ordinary simulation.
    await game.step({ frames: 25 });
    through = await moveAndSettle(
      game,
      { x: 60.7, y: -9.556, z: -102.4 },
      "inner threshold retry",
    );
  }
  assert.equal(through.blocked, false, `trial ${trial}: pass inner doors`);

  for (const [index, waypoint] of [
    { x: 59.5, y: -9.556, z: -102.4 },
    { x: 56, y: -9.556, z: -102.4 },
    { x: 51, y: -9.556, z: -102.4 },
    { x: 51, y: -9.556, z: -106 },
    { x: 49.8, y: -9.556, z: -106 },
    { x: 48, y: -9.556, z: -106 },
    { x: 45.3, y: -9.556, z: -106 },
  ].entries()) {
    await moveAndSettle(game, waypoint, `trial ${trial}: upper approach ${index}`);
  }
}

async function runTrial(trial: number, port: number): Promise<void> {
  await using game = await GameServer.launch({
    mission: "ops4.mis",
    port,
    echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
  });

  assert.equal((await game.load(ops4LiftSave!)).success, true);
  await game.step({ frames: 1 });

  const walls = await entity(game, LIFT_WALLS);
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

  await traverseToUpperLift(game, trial);

  // Ride down through the genuine upper button, walk clear of the deck, then
  // use the real lower button for an empty up/down cycle. This preserves the
  // exact clean campaign state that exposed the intermittent ascent failure.
  await frob(game, UPPER_BUTTON, `trial ${trial}: upper lift button`);
  await game.step({ frames: 300 });
  const lowerLift = await entity(game, LIFT);
  assert.ok(
    Math.abs(lowerLift.position[1] + 15.8) < 0.01,
    `trial ${trial}: lift must reach lower station, got ${lowerLift.position[1]}`,
  );
  await moveAndSettle(
    game,
    { x: 47.5, y: -14.356, z: -104.5 },
    `trial ${trial}: leave lower deck`,
  );
  await frob(game, LOWER_BUTTON, `trial ${trial}: send empty lift up`);
  await game.step({ frames: 300 });
  await frob(game, LOWER_BUTTON, `trial ${trial}: call empty lift down`);
  await game.step({ frames: 300 });

  // Board through the production bounded movement API and retain its required
  // eight-frame settle. No teleport or injected script message participates.
  await moveAndSettle(
    game,
    { x: 45.3, y: -14.356, z: -106 },
    `trial ${trial}: board exact center`,
  );
  const boarded = await game.player.position();
  assert.ok(
    Math.hypot(boarded.x - 45.3, boarded.z + 106) < 0.02,
    `trial ${trial}: bounded boarding must reach lift center: ${JSON.stringify(boarded)}`,
  );

  await frob(game, LOWER_BUTTON, `trial ${trial}: passenger ascent`);
  let elapsed = 0;
  let maxDrift = 0;
  for (const target of [0, 15, 30, 60, 75, 78, 80, 90, 120, 300]) {
    if (target > elapsed) await game.step({ frames: target - elapsed });
    elapsed = target;
    const player = await game.player.position();
    const lift = await entity(game, LIFT);
    const drift = Math.hypot(
      player.x - lift.position[0],
      player.z - lift.position[2],
    );
    maxDrift = Math.max(maxDrift, drift);
  }

  const finalPlayer = await game.player.position();
  const finalLift = await entity(game, LIFT);
  const supportOffset = finalPlayer.y - finalLift.position[1];
  assert.ok(
    finalLift.position[1] > -11.0,
    `trial ${trial}: lift should reach upper station, got ${finalLift.position[1]}`,
  );
  assert.ok(
    maxDrift < 0.05,
    `trial ${trial}: passenger drifted off center by ${maxDrift}`,
  );
  assert.ok(
    Math.abs(supportOffset - 1.444) < 0.05,
    `trial ${trial}: passenger lost support (offset ${supportOffset})`,
  );
}

/**
 * Regression for #592: a genuine bounded-board passenger must survive Ops4's
 * clean Lift 1 ascent. Mission object 129 authors a dimensionless sphere, and
 * the shaft lip sits within the controller's contact margin; neither may turn
 * a geometrically clear moving-support transfer into a lateral ejection.
 */
test(
  "Ops4 Lift 1 carries a centered passenger past dimensionless Lift 1 Walls",
  { skip: !saveE2eEnabled, timeout: 600_000 },
  async () => {
    const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8489);
    for (let trial = 1; trial <= 4; trial += 1) {
      await runTrial(trial, basePort + trial - 1);
    }
  },
);
