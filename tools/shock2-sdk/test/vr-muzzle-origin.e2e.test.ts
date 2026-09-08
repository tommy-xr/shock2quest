import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { cycleToWeapon } from "./helpers/weapon.js";
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
import type { Hand, Quat } from "./helpers/vr-hand.js";

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
  // No authored vhots: independently decoded front-cap vertices from 25AE
  // object polygons, with ND-arm material removed and bind translations applied.
  empgun_h: [-1.614333725, 0.100172064, -0.000514221],
  gren_h: [-1.105836868, 0.020605141, -0.000560760],
  fsn_h: [-1.244823456, 0.002128685, -0.003411102],
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
async function grabWith(game: GameServer, hand: Hand, target: Vec3): Promise<void> {
  await aimVrHandAt(game, target, 0.3, 0, 0, { hand });
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 8 });
}

/** The model's muzzle vhot as the wielding hand renders it: the left hand
 * draws the right-handed model reflected across the gun (its Z), muzzle
 * included, so a left-hand shot must leave the reflected point. */
function muzzleVhotFor(model: string, hand: Hand): Vec3 | undefined {
  const authored = MUZZLE_VHOT[model];
  if (!authored) return undefined;
  return hand === "left" ? [authored[0], authored[1], -authored[2]] : authored;
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
  hand: Hand = "right",
): Promise<{
  barrelDeviationDeg: number;
  lateralError: number;
  axialDistance: number;
  frameStep: number;
  distanceTravelled: number;
}> {
  await game.input.set(`${hand}_hand.position`, handLocalPosition);
  await game.input.set(`${hand}_hand.rotation`, quatFromTo([0, 0, -1], normalize(aimDirection)));
  await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 5 });

  const weapon = await entityTransform(game, weaponId);
  const vhot = muzzleVhotFor(weapon.model, hand);
  assert.ok(vhot, `VR should wield a view model with a known muzzle vhot, got "${weapon.model}"`);
  // Grip calibration can scale the displayed item (the laser is 1.3x).
  // The muzzle follows that rendered scale; projectile speed does not.
  const grip = (await game.info()).player.hand_grips.find(g => g.hand === hand && g.entity_id === weaponId)?.grip;
  // The psi amp retains its legacy hand mesh and has no glove item scaling.
  const itemScale = grip?.item_scale ?? 1;
  const scaledVhot: Vec3 = [vhot[0] * itemScale, vhot[1] * itemScale, vhot[2] * itemScale];
  const muzzle = add(weapon.position, quatRotate(weapon.rotation, scaledVhot));
  // The 25AE view models are authored with the barrel along the model's -X.
  const barrel = quatRotate(weapon.rotation, [-1, 0, 0]);

  const before = new Set((await game.entities.list()).entities.map((e) => e.id));
  await game.input.set(`${hand}_hand.trigger`, 1);
  await game.step({ frames: 1 });
  await game.input.set(`${hand}_hand.trigger`, 0);
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
    const laser = await cycleToWeapon(game, (e) => e.template_id === LASER_PISTOL);

    await grabWith(game, "right", laser.position as Vec3);
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

// A gun wielded in the LEFT hand draws the mirror image of its right-handed
// model (the baked hand becomes a left hand), and the muzzle reflects with it.
// Negative-first: unmirrored, the laser's muzzle sits 0.064 units the other
// side of the barrel plane, so the shot misses the reflected point by 0.128 -
// past the 0.05 tolerance below.
test(
  "a laser pistol wielded in the LEFT hand fires from its mirrored muzzle",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const laser = await cycleToWeapon(game, (e) => e.template_id === LASER_PISTOL);
    await grabWith(game, "left", laser.position as Vec3);
    const held = await game.info();
    assert.equal(held.player.wielded_entity_id, laser.id, "the LEFT hand slot must hold the pistol");
    assert.equal(held.player.right_hand_entity_id, null, "the right hand stays empty");

    const shot = await fireAndTrack(
      game,
      laser.id,
      [0, 1, -2],
      [-1, 0, 0],
      (e) => e.name === "Laser Shot",
      "left",
    );
    assertLeftTheMuzzle(shot, "left-hand bolt");
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

    await grabWith(game, "right", amp.position as Vec3);
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

for (const hand of ["left", "right"] as const) {
  test(`a ${hand}-held EMP rifle without vhots fires from the visible barrel cap`,
    { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" }, async () => {
      await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
      await game.step({ frames: 30 });
      const gun = await cycleToWeapon(game, e => e.name === "EMP Rifle", { settleFrames: 90 });
      await aimVrHandAt(game, gun.position as Vec3, 0.45, 1, 0, { hand });
      await game.step({ frames: 8 });
      const info = await game.info();
      assert.equal(hand === "right" ? info.player.right_hand_entity_id : info.player.wielded_entity_id, gun.id);
      const shot = await fireAndTrack(game, gun.id, [0, 1, -2], [-1, 0, 0], e => e.name === "EMP Shot", hand);
      assertLeftTheMuzzle(shot, `${hand} EMP bolt`);
    });
}

test("an EMP muzzle extending through the backstop cannot shoot through it",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const gun = await cycleToWeapon(game, e => e.name === "EMP Rifle", { settleFrames: 90 });
    await aimVrHandAt(game, gun.position as Vec3, 0.45, 1, 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, gun.id);
    // Backstop front is x=-11.5; the palm is in front and the barrel crosses it.
    await game.input.set("right_hand.position", [-10.5, 1, -2]);
    await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], [-1, 0, 0]));
    await game.step({ frames: 5 });
    const weapon = await entityTransform(game, gun.id);
    assert.ok(weapon.position[0] > -11.5 && weapon.position[0] < -10.6);
    const before = new Set((await game.entities.list()).entities.map(e => e.id));
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 0);
    let remaining = 0;
    for (let frame = 0; frame < 8; frame++) {
      const shots = (await game.entities.list()).entities.filter(e => !before.has(e.id) && e.name === "EMP Shot");
      // The sampled frame already integrated physics; allow its contact skin.
      for (const shot of shots) assert.ok(shot.position[0] >= -11.55, `bolt must not appear beyond the backstop: ${JSON.stringify(shot.position)}, weapon ${JSON.stringify(weapon.position)}, frame ${frame}`);
      remaining = shots.length;
      await game.step({ frames: 1 });
    }
    assert.equal(remaining, 0, "nearby backstop must absorb the bolt");
  });
