import assert from "node:assert/strict";
import { test } from "node:test";

import { rotateByInverse } from "../src/anim-metrics.js";
import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { fireOnce } from "./helpers/weapon.js";

// Earth Weapons Training's authored energy-weapon lesson:
//   1. Pick up the empty Laser Pistol with the normal reticle + use input.
//   2. Frob the real Recharging Station through the same input path.
//   3. Observe the station's activation sound and the laser charging to its
//      authored BaseGunDesc capacity.
//   4. Fire the charged laser through the normal trigger path.
//
// Runtime entity ids vary per launch, so the two mission objects are resolved
// by their stable mission-file object ids (exposed as `template_id`). Teleport
// is used only to stage the player beside each object; pickup and station
// activation are real reticle/squeeze interactions. In particular, this test
// does not use debug give, Reload, AdjustAmmo, or direct Recharge/Frob messages.
//
// Negative-first: before #531, EnergyWeapon was a NoopScript. The station
// activated and played audio, but the laser stayed at charge 0, so the
// capacity assertion failed and the real trigger remained a dry fire.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const LASER_PISTOL_OBJECT = 253;
const RECHARGING_STATION_OBJECT = 258;
const LASER_AUTHORED_CAPACITY = 100;

function ammoOf(detail: {
  properties: { name: string; value: string }[];
}): number {
  const ammo = detail.properties.find((property) => property.name === "Ammo");
  assert.ok(ammo, "laser should expose its GunState ammo/charge");
  return Number(ammo.value);
}

async function aimAt(
  game: GameServer,
  [targetX, targetY, targetZ]: Vec3,
): Promise<void> {
  const player = (await game.info()).player;
  const worldDelta: Vec3 = [
    targetX - player.position[0],
    targetY - (player.position[1] + PLAYER_EYE_HEIGHT_WORLD),
    targetZ - player.position[2],
  ];
  // `head.look` is pawn-local, while mission spawn markers may rotate the
  // pawn. Convert the desired world-space aim into that local frame first.
  const [dx, dy, dz] = rotateByInverse(worldDelta, player.rotation);
  const yawDeg = (Math.atan2(-dz, -dx) * 180) / Math.PI;
  const pitchDeg =
    (Math.atan2(-dy, Math.hypot(dx, dz)) * 180) / Math.PI;

  await game.input.set("head.look", [yawDeg, pitchDeg]);
  await game.step({ frames: 3 });
}

async function pulseUse(game: GameServer): Promise<void> {
  await game.input.set("right_hand.squeeze", 1.0);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.squeeze", 0.0);
  await game.step({ frames: 2 });
}

test(
  "earth.mis: the authored station recharges the normally acquired Laser Pistol",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8174),
    });
    await game.step({ frames: 5 });

    const laser = (await game.entities.byTemplate(LASER_PISTOL_OBJECT))[0];
    assert.ok(laser, "expected Earth Laser Pistol object 253");
    const station = (
      await game.entities.byTemplate(RECHARGING_STATION_OBJECT)
    )[0];
    assert.ok(station, "expected Earth Recharging Station object 258");

    const laserBefore = await game.entities.detail(laser.id);
    assert.equal(ammoOf(laserBefore), 0, "Earth laser should begin uncharged");
    const capacity = LASER_AUTHORED_CAPACITY;

    // Stand beside the laser, point the reticle at its live position, and use
    // it normally. Flat pickup wields the weapon in the player's left slot.
    await game.player.teleport({
      x: laser.position[0] + 3.0,
      y: laser.position[1] + 0.5,
      z: laser.position[2],
    });
    await game.step({ frames: 15 });
    const liveLaserPosition = (await game.entities.detail(laser.id)).position;
    await aimAt(game, liveLaserPosition);
    await pulseUse(game);
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      laser.id,
      "normal reticle/use pickup should wield the authored laser",
    );
    assert.ok(
      (await game.player.inventory()).items.some(
        (item) =>
          item.entity_id === laser.id &&
          item.name === "Laser Pistol" &&
          item.location === "left_hand",
      ),
      "the normally picked-up laser should be carried",
    );

    // A real trigger pull at zero must be a dry fire before the lesson.
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(laser.id)),
      0,
      "uncharged laser should remain dry",
    );

    // Stand beside the station and frob it with the same normal reticle/use
    // path. Snapshot audio first so the assertion proves this activation.
    await game.player.teleport({
      x: station.position[0] + 3.0,
      y: station.position[1] + 0.5,
      z: station.position[2],
    });
    await game.step({ frames: 15 });
    const liveStationPosition = (await game.entities.detail(station.id)).position;
    await aimAt(game, [
      liveStationPosition[0],
      liveStationPosition[1] + 1.5,
      liveStationPosition[2],
    ]);
    const audioBefore =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await pulseUse(game);
    await game.step({ frames: 10 });

    const activationSounds = (await game.audio.recent()).sounds.filter(
      (sound) =>
        sound.sequence > audioBefore &&
        sound.tags.some(
          ([tag, value]) => tag === "event" && value === "activate",
        ),
    );
    assert.ok(
      activationSounds.some(
        (sound) =>
          Math.hypot(
            sound.position[0] - liveStationPosition[0],
            sound.position[1] - liveStationPosition[1],
            sound.position[2] - liveStationPosition[2],
          ) < 0.1,
      ),
      "normal station Frob should play its positional activation sound",
    );
    assert.equal(
      ammoOf(await game.entities.detail(laser.id)),
      capacity,
      "station should charge the carried laser to authored capacity",
    );

    // Repeating the authored interaction at full charge is idempotent.
    await pulseUse(game);
    await game.step({ frames: 10 });
    assert.equal(
      ammoOf(await game.entities.detail(laser.id)),
      capacity,
      "a second station Frob should not exceed authored capacity",
    );

    // The charged weapon now fires normally and consumes charge.
    await fireOnce(game);
    const afterShot = ammoOf(await game.entities.detail(laser.id));
    assert.ok(
      afterShot >= 0 && afterShot < capacity,
      `a real laser shot should consume charge (${capacity} -> ${afterShot})`,
    );
  },
);
