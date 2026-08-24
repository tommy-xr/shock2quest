import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";
import {
  add,
  aimVrHandAt,
  dot,
  normalize,
  quatFromTo,
  quatRotate,
  scale,
  sub,
} from "./helpers/vr-hand.js";
import type { Quat } from "./helpers/vr-hand.js";

// A VR-wielded weapon must fire from its model's muzzle vhot, travelling along
// the rendered barrel. Both are checkable numerically: the runtime reports the
// weapon entity's world transform, and the muzzle vhot is a fixed point in the
// model, so the expected fire point and axis are known exactly.
//
// The laser pistol used to carry a 12 degree `projectile_rotation` fudge in
// vr_config - a leftover from the era when VR wielded the vhot-less `laser`
// world model - which skewed BOTH the spawn point and the travel direction off
// the barrel. Fired from a natural hold the bolt then crossed in front of the
// player and spanged on their own collider instead of reaching the wall.
//
// Requires a 25th Anniversary install (DARK_ASSET_PATH): the vhots below are
// the remastered `mods/sshock2ee.kpf` view models', which is what VR wields.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Weapon templates cycled by DebugCycleWeapon (mission_core DEBUG_WEAPONS). */
const LASER_PISTOL = -22;
const PSI_AMP = -247;

/** dark::SCALE_FACTOR - model units per world unit. */
const SCALE_FACTOR = 2.5;

/**
 * The muzzle vhot of each model, in world units (the model's vhot 0, read from
 * the 25AE `obj/*.bin` and divided by SCALE_FACTOR). Both sit at the model's
 * -X extreme, which is the authored barrel direction for these models.
 */
const MUZZLE_VHOT: Record<string, Vec3> = {
  lasehand: [-1.913 / SCALE_FACTOR, -0.0688 / SCALE_FACTOR, 0.16 / SCALE_FACTOR],
  amp_h: [-1.1072 / SCALE_FACTOR, 0.2719 / SCALE_FACTOR, -0.1199 / SCALE_FACTOR],
};

const len = (v: Vec3): number => Math.sqrt(dot(v, v));

/**
 * Stepped frames of flight between the shot and the first position that can be
 * sampled: the trigger frame spawns and integrates the projectile once, and one
 * more frame is stepped before the entity list is read.
 */
const FLIGHT_FRAMES_BEFORE_FIRST_SAMPLE = 2;

const angleBetweenDeg = (a: Vec3, b: Vec3): number =>
  (Math.acos(Math.max(-1, Math.min(1, dot(normalize(a), normalize(b))))) * 180) / Math.PI;

interface EntityTransform {
  position: Vec3;
  rotation: Quat;
  model: string;
}

async function entityTransform(game: GameServer, id: number): Promise<EntityTransform> {
  const detail = await game.entities.detail(id);
  const model =
    (detail.properties?.find((p) => p.name === "Model")?.value as string | undefined) ?? "";
  return {
    position: detail.position as Vec3,
    rotation: detail.rotation as Quat,
    model,
  };
}

/** Reach out with the production VR hand and squeeze to pick `target` up. */
async function grabWithRightHand(game: GameServer, target: Vec3): Promise<void> {
  await aimVrHandAt(game, target, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
}

/**
 * Hold the weapon at a known pose, pull the trigger, and report how the shot
 * compares to the muzzle and barrel the *rendered* weapon implies.
 *
 * The projectile is only observable after it has already flown, so the origin is
 * measured two ways against the ray it actually flew: how far the muzzle sits
 * OFF that ray (`lateralError` - a sideways spawn or a skewed heading), and how
 * far ALONG it the first sample is (`axialDistance`), which must match the
 * frames of flight that have elapsed - that is what catches a spawn pushed
 * forward or back down the barrel, which a lateral measure alone cannot see.
 */
async function fireAndTrack(
  game: GameServer,
  weaponId: number,
  handLocalPosition: Vec3,
  aimDirection: Vec3,
  projectileMatches: (entity: EntitySummary) => boolean,
): Promise<{
  barrelDeviationDeg: number;
  lateralError: number;
  axialDistance: number;
  frameStep: number;
  distanceTravelled: number;
}> {
  await game.input.set("right_hand.position", handLocalPosition);
  await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], normalize(aimDirection)));
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });

  const weapon = await entityTransform(game, weaponId);
  const vhot = MUZZLE_VHOT[weapon.model];
  assert.ok(vhot, `VR should wield a view model with a known muzzle vhot, got "${weapon.model}"`);
  const muzzle = add(weapon.position, quatRotate(weapon.rotation, vhot));
  // The 25AE view models are authored with the barrel along the model's -X.
  const barrel = quatRotate(weapon.rotation, [-1, 0, 0]);

  const before = new Set((await game.entities.list()).entities.map((e) => e.id));
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);
  // The trigger frame spawns the projectile and integrates it once; the first
  // loop iteration below steps once more before it can be observed. So the
  // first sample sits FLIGHT_FRAMES_BEFORE_FIRST_SAMPLE frames down the barrel
  // from where the shot actually started.

  const track: Vec3[] = [];
  for (let frame = 0; frame < 30; frame += 1) {
    await game.step({ frames: 1 });
    const shot = (await game.entities.list()).entities.find(
      (e) => !before.has(e.id) && projectileMatches(e),
    );
    if (shot) track.push(shot.position as Vec3);
  }
  assert.ok(track.length >= 3, "the trigger pull must spawn a travelling projectile");

  const travel = sub(track[track.length - 1], track[0]);
  const heading = normalize(travel);
  const muzzleToFirst = sub(track[0], muzzle);
  const axialDistance = dot(muzzleToFirst, heading);
  const lateral = sub(muzzleToFirst, scale(heading, axialDistance));

  return {
    barrelDeviationDeg: angleBetweenDeg(travel, barrel),
    lateralError: len(lateral),
    axialDistance,
    frameStep: len(sub(track[1], track[0])),
    distanceTravelled: len(travel),
  };
}

/** Assert a shot left the muzzle vhot travelling down the barrel. */
function assertLeftTheMuzzle(
  shot: Awaited<ReturnType<typeof fireAndTrack>>,
  what: string,
): void {
  assert.ok(
    shot.barrelDeviationDeg < 1,
    `the ${what} must travel along the rendered barrel, deviated ${shot.barrelDeviationDeg.toFixed(2)} degrees`,
  );
  assert.ok(
    shot.lateralError < 0.05,
    `the ${what}'s flight path must pass through the muzzle vhot, missing it sideways by ${shot.lateralError.toFixed(3)} world units`,
  );
  const expectedAxial = shot.frameStep * FLIGHT_FRAMES_BEFORE_FIRST_SAMPLE;
  assert.ok(
    Math.abs(shot.axialDistance - expectedAxial) < shot.frameStep * 0.25,
    `the ${what} must start at the muzzle: it was ${shot.axialDistance.toFixed(3)} units along the barrel after ` +
      `${FLIGHT_FRAMES_BEFORE_FIRST_SAMPLE} frames of flight, expected ${expectedAxial.toFixed(3)} ` +
      `(one frame is ${shot.frameStep.toFixed(3)})`,
  );
}

test(
  "a VR-wielded laser pistol fires from its muzzle vhot along the barrel",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // DebugCycleWeapon drops each weapon in front of the player in VR.
    let laser: EntitySummary | undefined;
    for (let cycle = 0; cycle < 12 && !laser; cycle += 1) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 10 });
      laser = (await game.entities.list()).entities.find((e) => e.template_id === LASER_PISTOL);
    }
    assert.ok(laser, "DebugCycleWeapon must spawn the Laser Pistol");

    await grabWithRightHand(game, laser.position as Vec3);
    const held = await game.info();
    assert.equal(held.player.right_hand_entity_id, laser.id, "the hand must hold the pistol");

    const shot = await fireAndTrack(
      game,
      laser.id,
      // Held out to the side of the body: a bolt that leaves the muzzle along
      // the barrel reaches the wall, one that veers sideways hits the player.
      [0, 1, -2],
      [-1, 0, 0],
      (e) => e.name === "Laser Shot",
    );

    assertLeftTheMuzzle(shot, "bolt");
    // A full-range flight: with the 12 degree skew the bolt used to spang on
    // the player's own collider a fraction of a unit out.
    assert.ok(
      shot.distanceTravelled > 5,
      `the bolt must clear the shooter, only travelled ${shot.distanceTravelled.toFixed(2)}`,
    );
  },
);

test(
  "a VR-wielded psi amp casts from its muzzle vhot along the barrel",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const amp = (await game.entities.list()).entities.find((e) => e.template_id === PSI_AMP);
    assert.ok(amp, "debug_psi must place the Psi Amp in the scene");

    await grabWithRightHand(game, amp.position as Vec3);
    const held = await game.info();
    assert.equal(held.player.right_hand_entity_id, amp.id, "the hand must hold the amp");

    // A pose that is not axis-aligned, so a wrong fire axis cannot coincide
    // with the right one.
    const shot = await fireAndTrack(
      game,
      amp.id,
      [0, 1, -2],
      [-1, 0.36, 0.84],
      (e) => e.name.startsWith("Cryo PSI"),
    );

    assertLeftTheMuzzle(shot, "cryo bolt");
  },
);
