// Capture a first-person VR clip for the README/website: up rick1's opening
// ladder hand over hand, hang one-handed at the first deck, draw the pistol
// (laser sight on) from the hip holster and drop the assassin standing there.
// Run from tools/shock2-sdk after npm run build:
// DARK_ASSET_PATH=/path/to/25AE node scripts/hero-ladder-draw.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  GameServer, handPoseAimedAt, ladderHoldPoints, lookQuat, pawnYawError, quatMultiply, quatRotate,
  recordClipInputs, sway, turnPawnToward, viewRight, vrReach,
} from "../dist/src/index.js";
import { installWeaponUpgrade } from "../dist/test/helpers/weapon-upgrades.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "screenshots/hero");
const clip = "rick1";
const assetRoot = process.env.DARK_ASSET_PATH;
assert.ok(assetRoot, "set DARK_ASSET_PATH to the 25AE remaster asset root");
for (const file of ["sshock2.kpf", ...["400", "patch_ext", "scp", "shtup", "sshock2ee"].map((name) => `mods/${name}.kpf`)]) {
  assert.ok((await stat(resolve(assetRoot, file))).size > 0, `missing ${file} from 25AE remaster assets`);
}
await mkdir(out, { recursive: true });

const MISSION = "rick1.mis";
// The assassin on the first deck up, leaning on the rail beside the opening
// ladder (mission object 1307; /v1/entities reports it as template_id).
const ASSASSIN = 1307;
// The pistol does ~4 a hit against the assassin's 60 hit points, so it is
// wounded this much off-camera; a few real hits then drop it in the clip.
const PRE_WOUND = 45;
// rick1 starts the player untrained, and an untrained pistol will not fire.
const STATS = { skills: { standard_weapons: 3 } };
// The README/website clips' field of view (hero-shots.mjs).
const FOV_DEG = 85;
// Engine lighting controls only; geometry and materials stay production.
const LIGHTING = { ambient_light_intensity: 1.6, level_light_intensity: 2.0, held_light_floor: 1.0 };
// Horizontal distance from the ladder's centre line to the climber's pawn.
const CLIMB_STANDOFF = 0.68;
// Climbing, the tracked head leans this far toward the ladder (pawn-local
// forward), bringing the shoulders - and so the rung above - within reach.
const CLIMB_LEAN = 0.15;
// Each stroke starts with the anchor hand's rung this far above the pawn: high
// enough that the lower hand's rung is in reach, low enough that after the
// pull the rung above the anchor is too.
const ANCHOR_OVER_PAWN = 1.0;
// Strokes in the clip, each one rung; the last ends with the eye this far
// above the assassin's origin - head just over the deck.
const STROKES = 3;
const END_EYE_OVER_ASSASSIN = 1.25;
// Overhand grips (see hero-climb.mjs).
const RUNG_PITCH = 45;
const RUNG_GRIP_UP = 0.06;
const RUNG_GRIP_OUT = 0.03;
// Each stroke: the anchor hand pulls one rung while the free hand reaches
// past it, arcing this far off the ladder.
const STROKE_FRAMES = 30;
const REACH_ARC = 0.08;
// Pawn-local controller pose of the free right hand while hanging.
const HANG_RIGHT = [0.24, 0.62, -0.28];
// The pawn turns toward the assassin until it is this far off-axis; the head
// covers the rest, looking over the shoulder.
const TURN_LEAVE_DEG = 35;
// Once turned, hang there looking at the assassin this long before drawing.
const HANG_BEAT_FRAMES = 20;
// The draw: frames to bring the hand to the holster, then to hold there
// once closed on the grip; meanwhile the eyes dip this far toward it.
const DRAW_FRAMES = 26;
const DRAW_HOLD_FRAMES = 6;
const GLANCE = 0.9;
// Frames to raise the pistol onto the target, then to settle before firing.
const RAISE_FRAMES = 22;
const SETTLE_FRAMES = 8;
// While aiming, the eyes sit this far right of the target (world units).
const LOOK_RIGHT = 0.4;
// Frames held on the death crumple before the clip ends.
const DEATH_HOLD_FRAMES = 110;
// Pistol at aim, from the eye: x right, y up, -z toward the target.
const AIM_RIGHT = [0.2, -0.27, -0.5];
// Trigger pulls after the pistol settles on target (seconds); irregular, as
// a person fires, but past the pistol's refire delay (~0.5 s; a pull sooner
// does nothing). The clip stops pulling once the assassin is down.
const SHOT_TIMES = [0, 0.6, 1.15, 1.8, 2.4, 2.95];
// Per-shot muzzle climb (world units at the target) and hand kick, decaying.
const RECOIL_RISE = 0.5;
const RECOIL_KICK = [0, 0.025, 0.04];
const RECOIL_DECAY_FRAMES = 6;
const FPS = 15;
const SIM_PER_SHOT = 60 / FPS;

const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const add = (a, b) => a.map((c, i) => c + b[i]);
const sub = (a, b) => a.map((c, i) => c - b[i]);
const scale = (a, s) => a.map((c) => c * s);
const normalize = (a) => scale(a, 1 / Math.hypot(...a));
const lerp = (a, b, t) => add(a, scale(sub(b, a), t));
const ease = (t) => t * t * (3 - 2 * t);
const conj = ([x, y, z, w]) => [-x, -y, -z, w];
const axisQuat = (axis, deg) => {
  const half = (deg * Math.PI) / 360;
  return [...scale(axis, Math.sin(half)), Math.cos(half)];
};
/** Normalized lerp between rotations, along the shorter arc. */
const nlerp = (a, b, t) => {
  const q = lerp(a, a.reduce((d, c, i) => d + c * b[i], 0) < 0 ? scale(b, -1) : b, t);
  return scale(q, 1 / Math.hypot(...q));
};
/** Overhand grip: knuckles `into` the ladder raised `pitch` degrees, palm rolled down. */
function overhand(hand, into, pitch) {
  const p = (pitch * Math.PI) / 180;
  return quatMultiply(lookQuat(add(scale(into, Math.cos(p)), [0, Math.sin(p), 0])),
    axisQuat([0, 0, 1], hand === "right" ? 90 : -90));
}
const property = (detail, name) => detail?.properties.find((p) => p.name === name)?.value;

// Smooth turning and no comfort vignette, without touching the user's settings.
const settingsDir = await mkdtemp(resolve(tmpdir(), "hero-settings-"));
process.env.SHOCK2_SETTINGS_PATH = resolve(settingsDir, "user-settings.json");
await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify({ vr: { turning: "Smooth", vignette: "Off" } }));
try {
  // AI timing jitters between runs, so a take can fail its checks: retry it.
  for (let take = 1; ; take++) {
    try {
      await capture();
      break;
    } catch (error) {
      if (!(error instanceof assert.AssertionError) || take === 3) throw error;
      console.log(`take ${take} failed (${error.message}); retrying`);
    }
  }
} finally {
  await rm(settingsDir, { recursive: true, force: true });
}

async function capture() {
  const framesDir = await mkdtemp(resolve(tmpdir(), "hero-ladder-draw-"));
  const game = await GameServer.launch({
    mission: MISSION,
    debugFlags: ["--vr", "--window-size", "1280x720"],
    repoRoot: root,
  });
  try {
    await take(game, framesDir);
  } finally {
    await game.shutdown();
    await rm(framesDir, { recursive: true, force: true });
  }
}

async function take(game, framesDir) {
  // Turning arms after a neutral stick frame on a fresh settings load.
  await game.step({ frames: 1 });
  // Invulnerable: the assassin's return fire otherwise reddens the clip.
  await game.devParams.set("cheat", 1);
  for (const [key, value] of Object.entries(LIGHTING)) await game.devParams.set(key, value);
  await game.devParams.set("fov_override_deg", FOV_DEG);
  await game.player.setStats(STATS);

  const assassin = (await game.entities.list({ filter: "Assassin", limit: 50 })).entities
    .find((e) => e.template_id === ASSASSIN);
  assert.ok(assassin, `no assassin ${ASSASSIN} in ${MISSION}`);
  await game.entities.sendMessage(assassin.id, { type: "Damage", amount: PRE_WOUND });
  // The ladder: the column of Rick Ladder 16 segments nearest the assassin.
  const segments = (await game.entities.list({ filter: "Rick Ladder 16", limit: 100 })).entities;
  const flat = (e) => Math.hypot(e.position[0] - assassin.position[0], e.position[2] - assassin.position[2]);
  const nearest = segments.reduce((a, b) => (flat(b) < flat(a) ? b : a));
  const column = segments.filter((e) => Math.hypot(e.position[0] - nearest.position[0], e.position[2] - nearest.position[2]) < 0.5);
  const ladders = await Promise.all(column.map((e) => game.physics.ladder(e.id)));
  // Climb the face that looks onto the deck.
  const axis = nearest.position;
  const n = ladders[0].normal;
  const toClimber = (assassin.position[0] - axis[0]) * n[0] + (assassin.position[2] - axis[2]) * n[2] >= 0 ? n : scale(n, -1);
  const into = scale(toClimber, -1);
  const spacing = ladders[0].rungs[1][0][1] - ladders[0].rungs[0][0][1];

  // Staging, outside the clip: at the ladder foot, facing it, pistol into the
  // right hip holster.
  const foot = Math.min(...ladders.flatMap((l) => l.rails.flat().map((p) => p[1])));
  const { player: spawn } = await game.info();
  const camY = spawn.camera_offset[1];
  const standX = axis[0] + toClimber[0] * CLIMB_STANDOFF;
  const standZ = axis[2] + toClimber[2] * CLIMB_STANDOFF;
  await game.player.teleport({ x: standX, y: foot + 1.3, z: standZ });
  await game.step({ frames: 20 });
  await turnPawnToward(game, [axis[0], foot, axis[2]], 2);
  await game.input.lookAtWorldPoint([axis[0], foot + 2, axis[2]]);
  await game.step({ frames: 60 });
  await game.input.set("right_hand.squeeze", 1);
  await game.player.spawnItem("Pistol", { hand: "right" });
  await game.step({ frames: 3 });
  let { player } = await game.info();
  const pistol = player.right_hand_entity_id;
  assert.ok(pistol, "the right hand must hold the pistol");
  // The real device chooser installs the laser sight and switches it on.
  await installWeaponUpgrade(game, "Laser");
  assert.equal(JSON.parse(property(await game.entities.detail(pistol), "WeaponUpgrades") ?? "{}").laser_enabled, true,
    "the pistol's laser sight must be installed and on");
  ({ player } = await game.info());
  const ammo = async () => Number(property(await game.entities.detail(pistol), "Ammo"));
  const right = await viewRight(game);
  const holsterSide = (i) => sub(player.hand_feedback.holsters.centers[i], player.position)
    .reduce((s, c, k) => s + c * right[k], 0);
  const slot = holsterSide(0) > holsterSide(1) ? 0 : 1;
  const toPawn = (world, pawn) => quatRotate(conj(pawn.rotation), sub(world, pawn.position));
  await game.input.set("right_hand.position", toPawn(player.hand_feedback.holsters.centers[slot], player));
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  ({ player } = await game.info());
  assert.equal(player.hand_feedback.holsters.items[slot], pistol, "the pistol must sit in the right hip holster");
  const roundsBefore = await ammo();

  // Onto the ladder below the deck, the right hand a rung above the left.
  // Its height once settled on the deck: the first listing reads it mid-settle.
  const deckY = (await game.entities.detail(assassin.id)).position[1];
  const endEye = deckY + END_EYE_OVER_ASSASSIN;
  const startEye = endEye - STROKES * spacing;
  const holds = Object.fromEntries(["left", "right"].map((hand) => [hand, ladders
    .flatMap((ladder) => ladderHoldPoints(ladder, hand, "rung", [standX, startEye, standZ], right))
    .map((p) => add(p, add([0, RUNG_GRIP_UP, 0], scale(toClimber, RUNG_GRIP_OUT))))
    .sort((a, b) => a[1] - b[1])]));
  // Hand world rotations, re-applied each frame so a gripping glove stays put
  // on its rung while the pawn turns under it.
  const handRotation = { left: overhand("left", into, RUNG_PITCH), right: overhand("right", into, RUNG_PITCH) };
  const applyRotations = async (pawn) => {
    for (const hand of ["left", "right"]) {
      if (handRotation[hand]) await game.input.set(`${hand}_hand.rotation`, quatMultiply(conj(pawn.rotation), handRotation[hand]));
    }
  };
  /** Target `point` with `hand` if the arm reaches it; false if it cannot. */
  const tryReach = async (hand, point) => {
    try {
      await vrReach(game, hand, point);
      return true;
    } catch (error) {
      if (!String(error.message).startsWith("vrReach:")) throw error;
      return false;
    }
  };
  await game.input.set("head.position", [0, camY, -CLIMB_LEAN]);
  // The right hand's rung that, after the strokes, puts the eye at endEye.
  // Index 1 up, so the left hand has a rung below.
  const wantY = startEye - camY + ANCHOR_OVER_PAWN;
  const startRung = holds.right.reduce((best, p, i) =>
    i > 0 && Math.abs(p[1] - wantY) < Math.abs(holds.right[best][1] - wantY) ? i : best, 1);
  const rung = { right: startRung, left: startRung - 1 };
  // Teleport a little high: the body falls into reach while the hands close.
  await game.player.teleport({ x: standX, y: holds.right[startRung][1] - ANCHOR_OVER_PAWN + 0.3, z: standZ });
  // The teleport lands on the next frame; grab before the fall builds.
  await game.step({ frames: 1 });
  await game.input.lookAtWorldPoint([axis[0], startEye, axis[2]]);
  await applyRotations((await game.info()).player);
  // Record from here, so the grips held into the clip start pressed.
  const inputs = recordClipInputs(game, { fps: FPS });
  for (const hand of ["left", "right"]) await game.input.set(`${hand}_hand.squeeze`, 0);
  const inReach = { left: 0, right: 0 };
  for (let frame = 0; frame < 10 && !(inReach.left > 1 && inReach.right > 1); frame++) {
    for (const hand of ["left", "right"]) {
      if (inReach[hand] || (await tryReach(hand, holds[hand][rung[hand]]))) inReach[hand]++;
    }
    await game.step({ frames: 1 });
  }
  for (const hand of ["left", "right"]) await game.input.set(`${hand}_hand.squeeze`, 1);
  await game.step({ frames: 1 });
  assert.equal((await game.info()).player.climb.grips.length, 2, "both hands must close on rungs");
  // Let staging HUD lines expire while hanging there.
  await game.step({ frames: 330 });
  // Settle the body to ANCHOR_OVER_PAWN under the right rung: lowering both
  // gripping controllers raises it.
  {
    const rise = holds.right[rung.right][1] - ANCHOR_OVER_PAWN - (await game.info()).player.position[1];
    const state = await game.input.state();
    const from = { left: state.left_hand.position, right: state.right_hand.position };
    for (let f = 1; f <= 20; f++) {
      for (const hand of ["left", "right"]) await game.input.set(`${hand}_hand.position`, add(from[hand], [0, -rise * f / 20, 0]));
      await game.step({ frames: 1 });
    }
    await game.step({ frames: 10 });
    assert.equal((await game.info()).player.climb.grips.length, 2, "both hands must hold through the settle");
  }

  // The clip. Each frame: ease the view direction toward `look.target`, step,
  // and capture every 4th frame (15 fps). Easing the direction, not a point,
  // keeps the head's turn rate even between near and far targets.
  let tick = 0;
  let t = 0;
  let phase = "climb";
  const samples = [];
  const look = { target: null, dir: null, ease: 0.15 };
  // The rendered eye, lean included.
  const eye = (p) => add(p.position, quatRotate(p.rotation, p.camera_offset));
  async function frame() {
    const { player } = await game.info();
    await applyRotations(player);
    const want = normalize(sub(look.target, eye(player)));
    look.dir = look.dir ? normalize(lerp(look.dir, want, look.ease)) : want;
    // A far point along the view: lookAtWorldPoint assumes an eye straight
    // over the pawn, and the climbing lean moves it off.
    await game.input.lookAtWorldPoint(add(add(eye(player), scale(look.dir, 10)), sway(7, t, 0.1, 0.25)));
    await game.step({ frames: 1 });
    t += 1 / 60;
    if (tick++ % SIM_PER_SHOT === 0) {
      const path = resolve(framesDir, `${String(samples.length).padStart(4, "0")}.png`);
      await game.screenshot(path, 1280);
      const { player: after } = await game.info();
      const target = await game.entities.detail(assassin.id);
      const laser = new Set((await game.scene.objects()).objects.map((o) => o.source)
        .filter((s) => s === "weapon_laser_core" || s === "weapon_laser_dot"));
      samples.push({ frame: tick - 1, image: path, phase, grips: after.climb.grips.map((g) => g.hand),
        beam: laser.has("weapon_laser_core"), dot: laser.has("weapon_laser_dot"),
        holding: after.right_hand_entity_id, health: after.hit_points,
        assassin: target && { y: target.position[1], hp: Number(property(target, "HitPoints")) } });
    }
  }
  const handWorld = async () => {
    const state = await game.input.state();
    return { left: state.left_hand.world_position, right: state.right_hand.world_position };
  };
  const targets = async () => {
    const detail = await game.entities.detail(assassin.id);
    if (!detail) return null;
    const point = (c) => detail.aim_points?.find((p) => p.classification === c)?.position;
    // The origin can sit well off the body, so it is the last resort.
    const torso = point("torso") ?? detail.position;
    return { head: point("head") ?? torso, torso,
      hp: Number(property(detail, "HitPoints")) };
  };

  // 1. Climb: the higher hand pulls one rung while the other reaches past it.
  const watchHands = async () => {
    const hands = await handWorld();
    return add(lerp(hands.left, hands.right, 0.5), [0, 0.1, 0]);
  };
  look.target = await watchHands();
  inputs.start();
  let anchor = "right";
  for (let stroke = 0; stroke < STROKES; stroke++) {
    const free = anchor === "right" ? "left" : "right";
    const goal = holds[free][rung[anchor] + 1];
    await game.input.set(`${free}_hand.squeeze`, 0);
    const from = (await handWorld())[free];
    const start = (await game.input.state())[`${anchor}_hand`].position;
    for (let f = 1; f <= STROKE_FRAMES; f++) {
      const s = ease(f / STROKE_FRAMES);
      await game.input.set(`${anchor}_hand.position`, add(start, [0, -spacing * s, 0]));
      // Out of reach for a frame, the hand holds its last target.
      await tryReach(free, add(lerp(from, goal, s), scale(toClimber, REACH_ARC * Math.sin(Math.PI * f / STROKE_FRAMES))));
      look.target = await watchHands();
      await frame();
    }
    assert.ok(await tryReach(free, goal), `the ${free} hand must reach its next rung`);
    await frame();
    await game.input.set(`${free}_hand.squeeze`, 1);
    await frame();
    const grips = (await game.info()).player.climb.grips;
    assert.ok(grips.some((g) => g.hand === free && g.kind === "ladder"), `the ${free} hand must close on a rung`);
    await game.input.set(`${anchor}_hand.squeeze`, 0);
    rung[free] = rung[anchor] + 1;
    anchor = free;
  }
  assert.equal(anchor, "left", "the climb must end on the left hand");

  // 2. Hang from the left hand: the right lets go and drops, the body swings
  // round toward the deck, the eyes find the assassin.
  phase = "hang";
  handRotation.right = null;
  const hangFrom = (await game.input.state()).right_hand.position;
  let aim = await targets();
  assert.ok(aim && aim.hp > 0, "the assassin must be alive on the deck");
  look.ease = 0.08;
  let turning = true;
  let turnedAt = null;
  for (let f = 0; f < 240 && (turnedAt === null || f - turnedAt < HANG_BEAT_FRAMES); f++) {
    const { player } = await game.info();
    await game.input.set("right_hand.position", lerp(hangFrom, HANG_RIGHT, ease(Math.min(1, f / 20))));
    await game.input.set("head.position", [0, camY, -CLIMB_LEAN * (1 - ease(Math.min(1, f / 30)))]);
    const error = pawnYawError(player.position, player.rotation, aim.head);
    turning &&= Math.abs(error) > (TURN_LEAVE_DEG * Math.PI) / 180;
    if (!turning && turnedAt === null) turnedAt = f;
    // Ease into the turn, and out of it as it nears the target.
    const push = turning ? Math.min(Math.min(1, f / 10), 0.25 + Math.abs(error) * 0.6) : 0;
    await game.input.set("right_hand.thumbstick", [Math.sign(error) * push, 0]);
    aim = (await targets()) ?? aim;
    look.target = f < 8 ? look.target : aim.head;
    await frame();
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  assert.ok(!turning, "the pawn must turn toward the assassin");

  // 3. Draw: eyes down to the hip as the right hand drops to the holster and
  // closes on the grip.
  phase = "draw";
  const drawFrom = (await game.input.state()).right_hand.position;
  const drawRotFrom = (await game.input.state()).right_hand.rotation;
  look.ease = 0.12;
  for (let f = 1; f <= DRAW_FRAMES + DRAW_HOLD_FRAMES; f++) {
    const { player } = await game.info();
    const holster = player.hand_feedback.holsters.centers[slot];
    const s = ease(Math.min(1, f / DRAW_FRAMES));
    await game.input.set("right_hand.position", lerp(drawFrom, toPawn(holster, player), s));
    await game.input.set("right_hand.rotation", nlerp(drawRotFrom, [0, 0, 0, 1], s));
    if (f === DRAW_FRAMES) await game.input.set("right_hand.squeeze", 1);
    aim = (await targets()) ?? aim;
    look.target = lerp(aim.head, holster, GLANCE);
    await frame();
  }
  ({ player } = await game.info());
  assert.equal(player.right_hand_entity_id, pistol, "the right hand must draw the pistol");
  assert.equal(player.hand_feedback.holsters.items[slot], null, "the holster must be empty after the draw");

  // 4. Raise onto the assassin, then fire until it drops. The eyes sit a
  // little right of it, so the pistol low on the right does not cover it.
  phase = "aim";
  const raiseFrom = { position: (await game.input.state()).right_hand.position, rotation: (await game.input.state()).right_hand.rotation };
  const shotFrames = new Set(SHOT_TIMES.map((at) => RAISE_FRAMES + SETTLE_FRAMES + Math.round(at * 60)));
  let lastShot = -Infinity;
  let pulls = 0;
  let downAt = null;
  const firstHp = aim.hp;
  for (let f = 1; ; f++) {
    const { player } = await game.info();
    aim = (await targets()) ?? aim;
    const kick = Math.exp(-(f - lastShot) / RECOIL_DECAY_FRAMES);
    const offset = add(add(AIM_RIGHT, sway(2, t, 0.008, 0.6)), scale(RECOIL_KICK, kick));
    const pose = handPoseAimedAt(player.position, player.rotation, player.camera_offset[1], aim.head, offset,
      add(aim.torso, [0, RECOIL_RISE * kick, 0]));
    const s = ease(Math.min(1, f / RAISE_FRAMES));
    await game.input.set("right_hand.position", lerp(raiseFrom.position, pose.position, s));
    await game.input.set("right_hand.rotation", nlerp(raiseFrom.rotation, pose.rotation, s));
    const toward = sub(aim.head, eye(player));
    look.target = add(aim.head, scale([-toward[2], 0, toward[0]], LOOK_RIGHT / Math.hypot(toward[0], toward[2])));
    if (f === RAISE_FRAMES + SETTLE_FRAMES) phase = "fire";
    if (downAt === null && shotFrames.has(f)) { lastShot = f; pulls++; }
    await game.input.set("right_hand.trigger", f - lastShot < 5 ? 1 : 0);
    await frame();
    if (downAt === null && aim.hp <= 0) downAt = f;
    // Hold on the death crumple, or give up after the last pull.
    if (downAt !== null && f - downAt > DEATH_HOLD_FRAMES) break;
    if (downAt === null && f > Math.max(...shotFrames) + 50) break;
  }
  await game.input.set("right_hand.trigger", 0);

  ({ player } = await game.info());
  const roundsAfter = await ammo();
  assert.ok(downAt !== null, `the assassin must go down (hp ${aim.hp} of ${firstHp} after ${pulls} pulls)`);
  assert.equal(roundsBefore - roundsAfter, pulls, "every trigger pull must fire a round");
  assert.ok(samples.filter((s) => s.phase !== "climb").every((s) => s.grips.includes("left")), "the left hand must hold the ladder throughout");
  assert.ok(samples.every((s) => s.health === samples[0].health), "no damage during the clip");
  const firing = samples.filter((s) => s.phase === "fire");
  assert.ok(firing.every((s) => s.beam && s.dot), "the laser beam and dot must show while firing");
  // AI varies: a take where the assassin leaves its deck (it can drop into
  // the shaft) frames the fight at the player's feet.
  assert.ok(samples.every((s) => !s.assassin || s.assassin.hp <= 0 || Math.abs(s.assassin.y - deckY) < 0.6),
    "the assassin must stay on its deck until it drops");
  const seconds = samples.length / FPS;
  assert.ok(seconds >= 5 && seconds <= 10, `expected a 5-10 second clip, got ${seconds.toFixed(1)}`);

  // The binary GameServer launched: an explicit override, else cargo's build.
  const runtime = process.env.SHOCK2_RUNTIME_BINARY
    || resolve(root, process.env.CARGO_TARGET_DIR ?? "target", "debug/debug_runtime");
  const runtimeSha256 = await sha256(runtime);
  await writeFile(resolve(out, `${clip}.inputs.json`), `${JSON.stringify({ ...inputs.timeline(), duration: seconds })}\n`);
  inputs.dispose();
  const input = resolve(framesDir, "%04d.png");
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(FPS), "-i", input,
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an",
    "-movflags", "+faststart", resolve(out, `${clip}.mp4`)]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(FPS), "-i", input,
    // README width; the view never holds still, so a smaller palette keeps it
    // near the other clips' size.
    "-vf", "scale=400:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
    resolve(out, `${clip}.gif`)]);
  // Poster: the first shot, muzzle flash on the assassin.
  const poster = samples.find((s) => s.phase === "fire");
  await copyFile(poster.image, resolve(out, `${clip}.png`));
  await writeFile(resolve(out, `${clip}.json`), JSON.stringify({
    mission: MISSION, presentation: "Vr", revision: git("rev-parse", "HEAD"),
    runtimeSha256, assetRoot,
    devParams: { cheat: 1, fov_override_deg: FOV_DEG, ...LIGHTING }, stats: STATS, settings: { vr: { turning: "Smooth", vignette: "Off" } },
    assassin: { object: ASSASSIN, preWound: PRE_WOUND, hp: [firstHp, aim.hp] }, pistol: { holsterSlot: slot, accessory: "Laser", pulls, rounds: [roundsBefore, roundsAfter] },
    ladder: { segments: ladders.map((l) => l.model), strokes: STROKES, startEye, endEye },
    video: { file: `${clip}.mp4`, fps: FPS, frames: samples.length }, inputs: `${clip}.inputs.json`, gif: `${clip}.gif`,
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ frames: samples.length, seconds, pulls, out }));
}
