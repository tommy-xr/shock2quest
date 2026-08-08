import assert from "node:assert/strict";
import { test } from "node:test";

import { AimOcclusionError, GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// Flat fast projectiles used to spawn SCALE_FACTOR (2.5 world units) ahead of
// the camera, then raycast back only SCALE_FACTOR * 0.25. The resulting ray
// began 1.875 units ahead of the eye, so an enemy already at melee range was
// behind the shot and could not be damaged (#669).
//
// Earth's stationary Training Droid makes the range boundary deterministic:
// both staging points are on its authored platform, and the target cannot
// chase the player between shots.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRAINING_DROID = 593;
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8469);

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(
    (candidate) => candidate.name === "HitPoints",
  );
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

async function stageAndAim(
  game: GameServer,
  targetId: number,
  horizontalDistance: number,
): Promise<number> {
  const [tx, ty, tz] = (await game.entities.detail(targetId)).position;
  await game.player.teleport({
    x: tx + horizontalDistance,
    y: ty + 1,
    z: tz,
  });
  await game.step({ frames: 60 });

  const settled = await game.player.position();
  await game.step({ frames: 10 });
  const stillSettled = await game.player.position();
  assert.ok(
    Math.abs(settled.y - stillSettled.y) < 0.02,
    `range staging must be collision-supported: ${JSON.stringify({
      settled,
      stillSettled,
    })}`,
  );

  const aim = await game.player.aimAt(targetId, {
    hitbox: "torso",
    visibility: "required",
  });
  assert.equal(aim.entity_id, targetId);
  assert.equal(aim.classification, "torso");
  assert.equal(aim.fallback_used, false);
  await game.step({ frames: 3 });
  return aim.visibility.target_distance;
}

test(
  "flat projectiles damage targets at melee-close and ordinary ranges",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: basePort,
    });
    await game.step({ frames: 5 });

    // SpawnDebugItem provisions and auto-wields the standard pistol.
    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 10 });
    assert.ok(
      (await game.info()).player.wielded_entity_id !== null,
      "the test needs a wielded flat-mode firearm",
    );

    const [droid] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(droid, "Earth should contain its authored Training Droid");

    let hp = hitPoints(await game.entities.detail(droid.id));
    const closeDistance = await stageAndAim(game, droid.id, 1.2);
    assert.ok(
      closeDistance < 1.8,
      `close-range regression must exercise the old blind zone, got ${closeDistance.toFixed(3)}`,
    );
    await fireOnce(game);
    await game.step({ frames: 5 });
    const afterClose = hitPoints(await game.entities.detail(droid.id));
    assert.ok(
      afterClose < hp,
      `a close-range torso shot should damage the droid (distance=${closeDistance.toFixed(3)}, hp=${hp}->${afterClose})`,
    );

    hp = afterClose;
    const ordinaryDistance = await stageAndAim(game, droid.id, 4);
    assert.ok(
      ordinaryDistance > 2.5,
      `ordinary-range control must be outside the old blind zone, got ${ordinaryDistance.toFixed(3)}`,
    );
    await fireOnce(game);
    await game.step({ frames: 5 });
    const afterOrdinary = hitPoints(await game.entities.detail(droid.id));
    assert.ok(
      afterOrdinary < hp,
      `an ordinary-range torso shot should still damage the droid (distance=${ordinaryDistance.toFixed(3)}, hp=${hp}->${afterOrdinary})`,
    );
  },
);

test(
  "a wall inside flat muzzle clearance still blocks a fast projectile",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
    });
    await game.step({ frames: 5 });

    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 5 });

    const known = new Set(
      (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(
        (entity) => entity.id,
      ),
    );
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 10 });
    const monster = (
      await game.entities.list({ filter: "OG-Pipe", limit: 50 })
    ).entities.find((entity) => !known.has(entity.id));
    assert.ok(monster, "SpawnDebugMonster should create a target");

    // Put the scene's wall at x=-12 between the camera at x=-14 and the
    // monster spawned near x=-4. Its near face is only ~1.5 units from the
    // camera: inside the 2.5-unit flat projectile spawn clearance. Starting a
    // fast ray at the spawned projectile would skip this cover.
    const player = await game.player.position();
    await game.player.teleport({ x: -14, y: player.y, z: 0 });
    await game.step({ frames: 5 });

    let aim;
    try {
      await game.player.aimAt(monster, {
        hitbox: "torso",
        visibility: "required",
      });
      assert.fail("the close wall should occlude the monster");
    } catch (error) {
      assert.ok(error instanceof AimOcclusionError);
      aim = error.result;
    }
    assert.equal(aim.visibility.state, "blocked");
    assert.ok(
      (aim.visibility.blocker?.distance ?? Number.POSITIVE_INFINITY) < 2.5,
      `the regression requires cover inside muzzle clearance: ${JSON.stringify(aim.visibility)}`,
    );
    // Required visibility intentionally refuses to move the production camera.
    // Aim along the rejected world ray anyway so firing exercises collision
    // against the reported blocker rather than a different direction.
    await game.input.set("head.rotation", aim.head_rotation);
    await game.step({ frames: 3 });

    const hpBefore = hitPoints(await game.entities.detail(monster.id));
    await fireOnce(game);
    await game.step({ frames: 5 });
    assert.equal(
      hitPoints(await game.entities.detail(monster.id)),
      hpBefore,
      "camera-origin collision must stop the shot at close cover",
    );
  },
);

test(
  "flat slow physical projectiles retain forward spawn clearance",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 2,
    });
    await game.step({ frames: 5 });

    // The fourth debug weapon is the laser pistol; Laser Shot velocity 40
    // follows the slow physical-projectile path rather than fast raycasting.
    for (let i = 0; i < 4; i++) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 3 });
    }
    const player = await game.player.position();

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    const shot = (
      await game.entities.list({ filter: "Laser Shot", limit: 20 })
    ).entities.find((entity) => entity.name === "Laser Shot");
    assert.ok(shot, "the slow laser projectile should remain live after one frame");
    assert.equal(
      (await game.physics.bodies({ entityId: shot.id })).bodies.length,
      1,
      "Laser Shot should use the physical projectile path",
    );
    assert.ok(
      player.x - shot.position[0] > 2,
      `slow flat projectiles must still spawn ahead of the player to clear their collider (player x=${player.x}, shot x=${shot.position[0]})`,
    );
    await game.input.set("right_hand.trigger", 0);
  },
);
