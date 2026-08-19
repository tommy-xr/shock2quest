import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";

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
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8102);

/** Weapon templates cycled by DebugCycleWeapon (mission_core DEBUG_WEAPONS). */
const LASER_PISTOL = -22;
const PSI_AMP = -247;

/** dark::SCALE_FACTOR - model units per world unit. */
const SCALE_FACTOR = 2.5;

/**
 * The muzzle vhot of each model, in world units (the model's vhot 0, read from
 * the 25AE `obj/*.bin` and divided by SCALE_FACTOR). Both sit at the model's
 * -X extreme, which is the authored barrel direction for Dark gun models.
 */
const MUZZLE_VHOT: Record<string, Vec3> = {
  lasehand: [-1.913 / SCALE_FACTOR, -0.0688 / SCALE_FACTOR, 0.16 / SCALE_FACTOR],
  amp_h: [-1.1072 / SCALE_FACTOR, 0.2719 / SCALE_FACTOR, -0.1199 / SCALE_FACTOR],
};

type Quat = [number, number, number, number];

const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const scale = (v: Vec3, s: number): Vec3 => [v[0] * s, v[1] * s, v[2] * s];
const dot = (a: Vec3, b: Vec3): number => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const len = (v: Vec3): number => Math.sqrt(dot(v, v));
const norm = (v: Vec3): Vec3 => scale(v, 1 / len(v));
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];

const qconj = ([x, y, z, w]: Quat): Quat => [-x, -y, -z, w];
const qmul = ([ax, ay, az, aw]: Quat, [bx, by, bz, bw]: Quat): Quat => [
  aw * bx + ax * bw + ay * bz - az * by,
  aw * by - ax * bz + ay * bw + az * bx,
  aw * bz + ax * by - ay * bx + az * bw,
  aw * bw - ax * bx - ay * by - az * bz,
];
const qnorm = (q: Quat): Quat => {
  const l = Math.sqrt(q.reduce((sum, v) => sum + v * v, 0));
  return q.map((v) => v / l) as Quat;
};
const qrot = (q: Quat, v: Vec3): Vec3 =>
  qmul(qmul(q, [...v, 0] as Quat), qconj(q)).slice(0, 3) as Vec3;
function qFromTo(from: Vec3, to: Vec3): Quat {
  const a = norm(from);
  const b = norm(to);
  const d = dot(a, b);
  if (d < -0.999999) {
    const axis = Math.abs(a[0]) < 0.9 ? norm(cross(a, [1, 0, 0])) : norm(cross(a, [0, 1, 0]));
    return [axis[0], axis[1], axis[2], 0];
  }
  const c = cross(a, b);
  return qnorm([c[0], c[1], c[2], 1 + d]);
}

const angleBetweenDeg = (a: Vec3, b: Vec3): number =>
  (Math.acos(Math.max(-1, Math.min(1, dot(norm(a), norm(b))))) * 180) / Math.PI;

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
  const snapshot = await game.info();
  const pawn = snapshot.player.position as Vec3;
  const pawnRotation = snapshot.player.rotation as Quat;
  const eye = add(pawn, [0, snapshot.player.camera_offset[1], 0]);
  const worldHand = sub(target, scale(norm(sub(target, eye)), 0.3));
  const inverse = qconj(pawnRotation);
  await game.input.set("right_hand.position", qrot(inverse, sub(worldHand, pawn)));
  await game.input.set(
    "right_hand.rotation",
    qnorm(qmul(inverse, qFromTo([0, 0, -1], norm(sub(target, worldHand))))),
  );
  await game.input.set("right_hand.trigger", 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
}

/**
 * Hold the weapon at a known pose, pull the trigger, and report where the
 * projectile appeared and which way it flew, alongside the muzzle/barrel the
 * rendered weapon says it should have used.
 */
async function fireAndTrack(
  game: GameServer,
  weaponId: number,
  handLocalPosition: Vec3,
  aimDirection: Vec3,
  projectileMatches: (entity: EntitySummary) => boolean,
): Promise<{
  muzzle: Vec3;
  barrel: Vec3;
  firstSeen: Vec3;
  travel: Vec3;
  distanceTravelled: number;
}> {
  await game.input.set("right_hand.position", handLocalPosition);
  await game.input.set("right_hand.rotation", qFromTo([0, 0, -1], norm(aimDirection)));
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 5 });

  const weapon = await entityTransform(game, weaponId);
  const vhot = MUZZLE_VHOT[weapon.model];
  assert.ok(
    vhot,
    `VR should wield a view model with a known muzzle vhot, got "${weapon.model}"`,
  );
  const muzzle = add(weapon.position, qrot(weapon.rotation, vhot));
  // Dark gun models are authored with the barrel along the model's -X axis.
  const barrel = qrot(weapon.rotation, [-1, 0, 0]);

  const before = new Set((await game.entities.list()).entities.map((e) => e.id));
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 1 });
  await game.input.set("right_hand.trigger", 0);

  const track: Vec3[] = [];
  for (let frame = 0; frame < 30; frame += 1) {
    await game.step({ frames: 1 });
    const shot = (await game.entities.list()).entities.find(
      (e) => !before.has(e.id) && projectileMatches(e),
    );
    if (shot) track.push(shot.position as Vec3);
  }
  assert.ok(track.length >= 2, "the trigger pull must spawn a travelling projectile");

  const travel = sub(track[track.length - 1], track[0]);
  return {
    muzzle,
    barrel,
    firstSeen: track[0],
    travel,
    distanceTravelled: len(travel),
  };
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

    const deviation = angleBetweenDeg(shot.travel, shot.barrel);
    assert.ok(
      deviation < 1,
      `the bolt must travel along the rendered barrel, deviated ${deviation.toFixed(2)} degrees`,
    );

    // The projectile is first observed one frame after the shot, so compare its
    // back-projection to the muzzle rather than the raw first sample.
    const origin = sub(shot.firstSeen, scale(norm(shot.travel), len(sub(shot.firstSeen, shot.muzzle))));
    const originError = len(sub(origin, shot.muzzle));
    assert.ok(
      originError < 0.05,
      `the bolt must leave the muzzle vhot, missed it by ${originError.toFixed(3)} world units`,
    );

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
      port: basePort + 1,
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

    const deviation = angleBetweenDeg(shot.travel, shot.barrel);
    assert.ok(
      deviation < 1,
      `the cryo bolt must travel along the barrel, deviated ${deviation.toFixed(2)} degrees`,
    );

    const origin = sub(shot.firstSeen, scale(norm(shot.travel), len(sub(shot.firstSeen, shot.muzzle))));
    const originError = len(sub(origin, shot.muzzle));
    assert.ok(
      originError < 0.05,
      `the cryo bolt must leave the muzzle vhot, missed it by ${originError.toFixed(3)} world units`,
    );
  },
);
