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
// The ordinary matrix requires a 25th Anniversary install (DARK_ASSET_PATH):
// its viewmodel geometry comes from `mods/sshock2ee.kpf`. The separately gated
// classic case deliberately runs against an unpacked legacy install.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const classicE2eEnabled = e2eEnabled && process.env.SHOCK2_CLASSIC_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8102);

/** Weapon templates cycled by DebugCycleWeapon (mission_core DEBUG_WEAPONS). */
const LASER_PISTOL = -22;
const EMP_RIFLE = -23;
const GRENADE_LAUNCHER = -21;
const PSI_AMP = -247;

/** dark::SCALE_FACTOR - model units per world unit. */
const SCALE_FACTOR = 2.5;

/**
 * The resolved muzzle point of each model, in world units. Vhot-carrying
 * models use the authored point; `empgun_h` has none, so its point is the
 * centre of the -X face of its decoded model bounds. All three are authored
 * barrel-along -X.
 */
const MUZZLE_POINT: Record<string, Vec3> = {
  lasehand: [-1.913 / SCALE_FACTOR, -0.0688 / SCALE_FACTOR, 0.16 / SCALE_FACTOR],
  empgun_h: [-1.5118, 0, 0],
  amp_h: [-1.1072 / SCALE_FACTOR, 0.2719 / SCALE_FACTOR, -0.1199 / SCALE_FACTOR],
  // Classic-install world model: no vhot, so the centre of its -Z bounds face.
  gren_w: [0, 0, -1.1999],
};

const BARREL_AXIS: Record<string, Vec3> = {
  gren_w: [0, 0, -1],
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
  const muzzlePoint = MUZZLE_POINT[weapon.model];
  assert.ok(
    muzzlePoint,
    `VR should wield a view model with known muzzle geometry, got "${weapon.model}"`,
  );
  const muzzle = add(weapon.position, quatRotate(weapon.rotation, muzzlePoint));
  const barrel = quatRotate(weapon.rotation, BARREL_AXIS[weapon.model] ?? [-1, 0, 0]);

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

  const firstStep = sub(track[1], track[0]);
  const travel = sub(track[track.length - 1], track[0]);
  // Initial direction is the barrel rule under test. Using the whole track
  // would misclassify a correctly-launched physical grenade after gravity has
  // bent its later trajectory.
  const heading = normalize(firstStep);
  const muzzleToFirst = sub(track[0], muzzle);
  const axialDistance = dot(muzzleToFirst, heading);
  const lateral = sub(muzzleToFirst, scale(heading, axialDistance));

  return {
    barrelDeviationDeg: angleBetweenDeg(firstStep, barrel),
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
      port: basePort,
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
  "a vhotless VR EMP rifle fires from its model bounds and clears a close hold",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    let emp: EntitySummary | undefined;
    for (let cycle = 0; cycle < 12 && !emp; cycle += 1) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 10 });
      emp = (await game.entities.list()).entities.find((e) => e.template_id === EMP_RIFLE);
    }
    assert.ok(emp, "DebugCycleWeapon must spawn the EMP Rifle");

    await grabWithRightHand(game, emp.position as Vec3);
    const held = await game.info();
    assert.equal(held.player.right_hand_entity_id, emp.id, "the hand must hold the EMP rifle");

    const shot = await fireAndTrack(
      game,
      emp.id,
      // The grip itself is in the player capsule. On the old fallback the EMP
      // shot spawned there and detonated immediately; the bounds-derived tip
      // sits outside the body and visibly launches down the barrel.
      [0, 1, 0],
      [-1, 0, 0],
      (e) => e.name === "EMP Shot",
    );

    assertLeftTheMuzzle(shot, "EMP pulse");
    assert.ok(
      shot.distanceTravelled > 5,
      `the EMP pulse must clear the close-held shooter, only travelled ${shot.distanceTravelled.toFixed(2)}`,
    );
  },
);

test(
  "classic VR: a Z-long vhotless grenade launcher fires down its rendered barrel",
  {
    skip: classicE2eEnabled
      ? false
      : "set SHOCK2_E2E=1 and SHOCK2_CLASSIC_E2E=1 with classic assets to run",
  },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 3,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    let launcher: EntitySummary | undefined;
    for (let cycle = 0; cycle < 12 && !launcher; cycle += 1) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 10 });
      launcher = (await game.entities.list()).entities.find(
        (e) => e.template_id === GRENADE_LAUNCHER,
      );
    }
    assert.ok(launcher, "DebugCycleWeapon must spawn the Grenade Launcher");

    await grabWithRightHand(game, launcher.position as Vec3);
    const held = await game.info();
    assert.equal(
      held.player.right_hand_entity_id,
      launcher.id,
      "the hand must hold the grenade launcher",
    );
    assert.equal((await entityTransform(game, launcher.id)).model, "gren_w");

    const shot = await fireAndTrack(
      game,
      launcher.id,
      [0, 1, -2],
      // The classic grip rotates model -Z onto hand +X. Turning the hand back
      // maps that rendered barrel onto the debug scene's clear -X lane.
      [0, 0, 1],
      (e) => e.name.includes("Grenade Proj"),
    );

    assertLeftTheMuzzle(shot, "classic grenade");
  },
);

test(
  "a VR-wielded psi amp casts from its muzzle vhot along the barrel",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: basePort + 2,
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
