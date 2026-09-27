// Reproducible README/website hero stills. Built SDK; run from tools/shock2-sdk:
//   npm run build && node scripts/hero-shots.mjs [--only hydro1] [--out <dir>]
//   node scripts/hero-shots.mjs --replay <rec-*.jsonl> --name climb   # a recorded session as a GIF
// Each shot boots its mission fresh in VR presentation (no screen-space HUD),
// stands the player at `player` with a loadout in hand, aimed at the nearest
// entity matching `subject` - a first-person VR view - and, for shots with a
// `clip` or `melee`, records it as a GIF and MP4. Each output gets a
// `<name>.json` provenance file. AI wanders, so framing is only coarsely
// reproducible.
import { execFileSync } from "node:child_process";
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  aimHandsAt,
  attachSupportHand,
  faceTarget,
  GameServer,
  quatMultiply,
  quatRotate,
  sampleTrack,
  setHandWorldPose,
  sway,
} from "../dist/src/index.js";

// Hand offsets from the eye: x right, y up, -z toward the target.
const AIM_RIGHT = [0.1, -0.18, -0.55];
const AIM_LEFT = [-0.1, -0.2, -0.52];
const REST_LEFT = [-0.18, -0.35, -0.4];
const REST_RIGHT = [0.18, -0.4, -0.35];
// A melee weapon held up and ready, in frame.
const READY_LEFT = [-0.16, -0.24, -0.45];

const SHOTS = [
  {
    // Two-handed assault rifle, at the start by the egg pods.
    name: "hydro1",
    mission: "hydro1.mis",
    player: [15.8, 0.84, 52.4],
    subject: "Floor Pod",
    loadout: { right: -18 },
    hands: { right: [0.16, -0.12, -0.34] },
    twoHanded: "right",
    stats: { strength: 5, skills: { standard_weapons: 6 } },
  },
  {
    // Weapon + melee.
    name: "medsci1",
    mission: "medsci1.mis",
    player: [6.8, 0.7, -35.7],
    subject: "OG-Pipe",
    loadout: { right: "Pistol", left: "Wrench" },
    hands: { right: AIM_RIGHT, left: AIM_LEFT },
  },
  {
    // Psi amp + weapon.
    name: "ops2",
    mission: "ops2.mis",
    player: [65.5, -7.3, 137.5],
    subject: "Protocol Droid",
    loadout: { right: "Laser Pistol", left: -247 },
    hands: { right: [0.2, -0.2, -0.5], left: [-0.14, -0.22, -0.5] },
    // Psi needs a psionic character; the amp casts Cryokinesis on release.
    stats: { psionic_ability: 6, psi_tier: 5 },
    // Cast from the amp, then fire the laser pistol twice.
    clip: {
      seconds: 2.2,
      hands: {
        left: [
          { t: 0, value: [-0.2, -0.35, -0.4] },
          { t: 0.5, value: [-0.14, -0.22, -0.5] },
        ],
        right: [
          { t: 0, value: REST_RIGHT },
          { t: 0.9, value: [0.2, -0.2, -0.5] },
        ],
      },
      trigger: { left: [0.7], right: [1.3, 1.7] },
    },
  },
  {
    // Weapon + melee in motion: pistol held, one wrench swing.
    name: "medsci1-melee",
    mission: "medsci1.mis",
    player: [16, 1, 17],
    heading: [-1, 0, 0],
    // A pipe hybrid, spawned in front of the player by SpawnDebugMonster: the
    // mission's own wander in and out of this quiet room.
    spawnMonster: -397,
    loadout: { right: "Pistol", left: "Wrench" },
    hands: { right: AIM_RIGHT, left: READY_LEFT },
    melee: true,
  },
  {
    // Single weapon.
    name: "rec1",
    mission: "rec1.mis",
    player: [-10.6, -2.9, -101.0],
    subject: "Red Monkey",
    loadout: { right: "Pistol" },
    hands: { right: AIM_RIGHT, left: REST_LEFT },
    // Raise the pistol from the hip, settle, fire twice.
    clip: {
      seconds: 2.0,
      hands: {
        right: [
          { t: 0, value: REST_RIGHT },
          { t: 0.6, value: AIM_RIGHT },
        ],
        left: [{ t: 0, value: REST_LEFT }],
      },
      trigger: { right: [1.0, 1.5] },
    },
  },
];

const { values } = parseArgs({
  options: {
    out: { type: "string" },
    only: { type: "string" },
    "max-width": { type: "string", default: "1280" },
    fov: { type: "string", default: "85" },
    "gif-width": { type: "string", default: "400" },
    "video-width": { type: "string", default: "960" },
    replay: { type: "string" },
    name: { type: "string", default: "replay" },
  },
});
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = values.out ? resolve(values.out) : resolve(repoRoot, "screenshots/hero");
await mkdir(out, { recursive: true });
// The source revision, read before captures overwrite tracked media.
const git = (...args) => execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
const source = {
  revision: git("rev-parse", "HEAD"),
  dirty: git("status", "--porcelain", "--untracked-files=no") !== "",
};

if (values.replay) {
  await renderRecording(resolve(values.replay), values.name);
  process.exit(0);
}

const shots = values.only ? SHOTS.filter((s) => s.name === values.only) : SHOTS;
if (shots.length === 0) throw new Error(`no shot named '${values.only}'`);

for (const shot of shots) {
  // AI timing jitters between runs, so a take can fail its checks: retry it.
  for (let take = 1; ; take++) {
    try {
      await captureShot(shot);
      break;
    } catch (error) {
      if (!(error instanceof assert.AssertionError) || take === 3) throw error;
      console.log(`${shot.name}: take ${take} failed (${error.message}); retrying`);
    }
  }
}

async function captureShot(shot) {
  const game = await GameServer.launch({
    mission: shot.mission,
    debugFlags: ["--vr", "--window-size", "1920x1080"],
    repoRoot,
  });
  try {
    if (shot.stats) await game.player.setStats(shot.stats);
    let subject;
    if (shot.subject) {
      const { entities } = await game.entities.list({ filter: shot.subject, limit: 50 });
      const dist = (e) => Math.hypot(...e.position.map((v, i) => v - shot.player[i]));
      subject = entities.sort((a, b) => dist(a) - dist(b))[0];
      if (!subject) throw new Error(`${shot.name}: no '${shot.subject}' in ${shot.mission}`);
    }
    const heading = shot.heading ?? subject.position.map((v, i) => v - shot.player[i]);

    // Square up at the spawn point, out of the subject's sight, along the
    // shot's heading (settling takes seconds - long enough to draw an attack).
    const spawn = (await game.info()).player.position;
    await faceTarget(game, spawn.map((v, i) => v + heading[i]));
    const [x, y, z] = shot.player;
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 5 });
    if (shot.spawnMonster) subject = await spawnMonster(game, shot.spawnMonster);
    // Aim where the subject is now; it may have moved while we squared up.
    const target = (await aimPoints(game, subject.id))("torso");
    await aimHandsAt(game, target, shot.hands);
    await game.step({ frames: 2 });
    // Hold the grips: a VR hand releases whatever it holds when it lets go.
    for (const [hand, template] of Object.entries(shot.loadout)) {
      await game.input.set(`${hand}_hand.squeeze`, 1);
      await game.player.spawnItem(template, { hand });
    }
    await game.step({ frames: 3 });
    const { player } = await game.info();
    const held = { left: player.wielded_entity_id, right: player.right_hand_entity_id };
    for (const hand of Object.keys(shot.loadout)) {
      if (held[hand] == null) throw new Error(`${shot.name}: ${hand} hand holds nothing`);
    }
    if (shot.twoHanded) await attachSupportHand(game, shot.twoHanded);

    await game.devParams.set("fov_override_deg", Number(values.fov));
    await game.step({ frames: 2 });
    const path = resolve(out, `${shot.name}.png`);
    const result = await game.screenshot(path, Number(values["max-width"]));
    console.log(`${shot.name}: ${result.full_path} ${result.resolution.join("x")}`);
    const provenance = {
      mission: shot.mission,
      player: shot.player,
      subject: shot.subject ?? `template ${shot.spawnMonster}`,
      loadout: shot.loadout,
      stats: shot.stats,
      fovDeg: Number(values.fov),
      still: { file: `${shot.name}.png`, resolution: result.resolution },
    };
    if (shot.clip) provenance.clip = await recordClip(game, shot, subject.id);
    if (shot.melee) provenance.melee = await recordMelee(game, shot, subject.id, held);
    await writeProvenance(shot.name, provenance);
  } finally {
    await game.shutdown();
  }
}

/** Look up the subject's live aim points by class, falling back to the torso,
 * then its origin (which can sit well off its body - a monkey's is above its
 * back). */
async function aimPoints(game, id) {
  const detail = await game.entities.detail(id);
  const find = (c) => detail.aim_points?.find((p) => p.classification === c)?.position;
  return (classification) => find(classification) ?? find("torso") ?? detail.position;
}

/**
 * Play `shot.clip` one 60 Hz frame at a time - eased hand tracks plus seeded
 * sway, aimed at the (moving) subject, trigger pulls at the listed times -
 * capturing every 4th frame (15 fps) into `<name>.gif`.
 */
async function recordClip(game, shot, subjectId) {
  const { clip } = shot;
  const frames = Math.round(clip.seconds * 60);
  const sink = await frameSink(game, shot.name);
  try {
    for (let frame = 0; frame < frames; frame++) {
      const t = frame / 60;
      const hands = Object.fromEntries(
        Object.entries(clip.hands).map(([hand, keys]) => [hand, sampleTrack(keys, t)]),
      );
      await trackSubject(game, await aimPoints(game, subjectId), t, hands);
      for (const [hand, pulls] of Object.entries(clip.trigger ?? {})) {
        const pulled = pulls.some((at) => t >= at && t < at + 0.12);
        await game.input.set(`${hand}_hand.trigger`, pulled ? 1 : 0);
      }
      await sink.step();
    }
    return sink.finish();
  } finally {
    await sink.dispose();
  }
}

/** Spawn `template` in front of the player (SpawnDebugMonster) and return it. */
async function spawnMonster(game, template) {
  const before = new Set((await game.entities.byTemplate(template)).map((e) => e.id));
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 1 });
  const spawned = (await game.entities.byTemplate(template)).find((e) => !before.has(e.id));
  if (!spawned) throw new Error(`SpawnDebugMonster did not spawn template ${template}`);
  return spawned;
}

/**
 * Aim at `point` (from `aimPoints`) for time `t`: eyes on its head (a charging
 * creature's torso would pitch the view into the floor), `hands` - offsets
 * from the eye - on its torso, all with seeded drift and tremor. View pitch is
 * capped so a creature in the face or falling at the feet doesn't swing it.
 * Returns the pawn and eye it aimed from.
 */
async function trackSubject(game, point, t, hands) {
  const drift = sway(1, t, 0.04);
  const { player } = await game.info();
  const eye = player.position.map((v, i) => v + (i === 1 ? player.camera_offset[1] : 0));
  const head = point("head").map((v, i) => v + drift[i]);
  const flat = Math.hypot(head[0] - eye[0], head[2] - eye[2]);
  const maxRise = flat * Math.tan((25 * Math.PI) / 180);
  const look = [head[0], eye[1] + Math.max(-maxRise, Math.min(maxRise, head[1] - eye[1])), head[2]];
  const body = point("torso").map((v, i) => v + drift[i]);
  const trembling = Object.fromEntries(
    Object.entries(hands).map(([hand, offset]) => {
      const tremor = sway(hand === "right" ? 2 : 3, t, 0.008, 0.6);
      return [hand, offset.map((v, i) => v + tremor[i])];
    }),
  );
  await aimHandsAt(game, look, trembling, body);
  return { player, eye };
}

/**
 * Pistol held, one wrench swing through the spawned hybrid's torso once it
 * is in reach. Damage comes only from tracked-hand input, and the hit is
 * asserted, so a take where the swing misses fails instead of writing a
 * misleading clip.
 */
async function recordMelee(game, shot, subjectId, held) {
  // A missing property is a broken setup, not a bad take: throw, don't assert.
  const property = async (id, name) => {
    const found = (await game.entities.detail(id)).properties.find((p) => p.name === name);
    if (!found) throw new Error(`entity ${id} has no ${name}`);
    return Number(found.value);
  };
  const hp = () => property(subjectId, "HitPoints");
  const sink = await frameSink(game, shot.name);
  let t = 0;
  let point;
  // Track the subject for `seconds`.
  const hold = async (seconds, hands) => {
    for (const end = t + seconds; t < end; t += 1 / 60) {
      point = await aimPoints(game, subjectId);
      await trackSubject(game, point, t, hands);
      await sink.step();
    }
  };
  try {
    // Both weapons up while it comes on.
    await hold(0.6, shot.hands);

    // Step in to wrench range: the hybrid can hold at its own pipe's reach,
    // beyond the wrench's.
    const reach = async () => {
      const [monster, { player }] = [await game.entities.detail(subjectId), await game.info()];
      return Math.hypot(
        monster.position[0] - player.position[0],
        monster.position[2] - player.position[2],
      );
    };
    for (let frame = 0; frame < 240 && (await reach()) > 1.5; frame++) {
      await game.input.set("right_hand.thumbstick", [0, 0.6]);
      await hold(1 / 60, shot.hands);
    }
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const beforeSwingHp = await hp();

    // Overhead wrench swing: raise it over the shoulder, then chop down through
    // the torso. The wrench runs up the fist's +Y, its head ~0.75 out; pitching
    // the fist about its X axis from +30deg (tilted back) to -80deg (pointing
    // forward, slightly down) sweeps the head forward and down, in view.
    const windUp = 18;
    const chop = 16;
    const followThrough = 12;
    const total = windUp + chop + followThrough;
    for (let frame = 0; frame < total; frame++, t += 1 / 60) {
      point = await aimPoints(game, subjectId);
      const { player, eye } = await trackSubject(game, point, t, { right: shot.hands.right });
      const torso = point("torso");
      const toward = torso.map((v, i) => v - eye[i]);
      const yaw = Math.atan2(-toward[0], -toward[2]);
      const yawQuat = [0, Math.sin(yaw / 2), 0, Math.cos(yaw / 2)];
      // Swing so the head ends at the torso's horizontal distance.
      const lunge = -Math.min(0.75, Math.max(0.35, Math.hypot(toward[0], toward[2]) - 0.74));
      const struck = [-0.05, -0.22, lunge];
      const pitch = sampleTrack(
        [
          { t: 0, value: 0 },
          { t: windUp, value: 30 },
          { t: windUp + chop, value: -80 },
          { t: total - 1, value: -70 },
        ],
        frame,
      );
      const pivot = sampleTrack(
        [
          { t: 0, value: READY_LEFT },
          { t: windUp, value: [-0.2, -0.12, -0.35] },
          { t: windUp + chop, value: struck },
          { t: total - 1, value: struck },
        ],
        frame,
      );
      const radians = (pitch * Math.PI) / 180;
      const rotation = quatMultiply(yawQuat, [Math.sin(radians / 2), 0, 0, Math.cos(radians / 2)]);
      const hand = eye.map((v, i) => v + quatRotate(yawQuat, pivot)[i]);
      await setHandWorldPose(game, player, "left", hand, rotation);
      await sink.step();
    }
    const afterSwingHp = await hp();
    assert.ok(
      afterSwingHp < beforeSwingHp,
      `the wrench must connect (${beforeSwingHp} -> ${afterSwingHp})`,
    );

    // Recover: the wrench back to ready beside the pistol.
    await hold(0.8, shot.hands);
    const { player } = await game.info();
    assert.equal(player.life_state, "alive");
    assert.equal(player.right_hand_entity_id, held.right, "the pistol must still be held");
    assert.equal(player.wielded_entity_id, held.left, "the wrench must still be held");
    return {
      ...sink.finish(),
      hp: [beforeSwingHp, afterSwingHp],
    };
  } finally {
    await sink.dispose();
  }
}

/**
 * Collects a clip one 60 Hz frame at a time, capturing every 4th (15 fps);
 * `finish` writes `<out>/<name>.gif` and `.mp4`.
 */
async function frameSink(game, name) {
  const dir = await mkdtemp(resolve(tmpdir(), `hero-${name}-`));
  let frame = 0;
  let captured = 0;
  return {
    async step() {
      await game.step({ frames: 1 });
      if (frame++ % 4 === 0) await this.capture();
    },
    async capture() {
      const file = resolve(dir, `${String(captured++).padStart(4, "0")}.png`);
      await game.screenshot(file, Number(values["video-width"]));
    },
    finish: () => writeMedia(dir, name, captured),
    dispose: () => rm(dir, { recursive: true, force: true }),
  };
}

/** Assemble `dir/0000.png...` (15 fps) into `<out>/<name>.gif` and `.mp4`. */
function writeMedia(dir, name, frames) {
  const input = ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(dir, "%04d.png")];
  const gifWidth = Number(values["gif-width"]);
  execFileSync("ffmpeg", [
    ...input,
    "-vf",
    `scale=${gifWidth}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4`,
    resolve(out, `${name}.gif`),
  ]);
  // H.264 needs even dimensions.
  execFileSync("ffmpeg", [
    ...input,
    "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    resolve(out, `${name}.mp4`),
  ]);
  console.log(`${name}: ${resolve(out, name)}.{gif,mp4}`);
  return { files: [`${name}.gif`, `${name}.mp4`], frames, fps: 15 };
}

/** Write `<out>/<name>.json`: what was captured, from which revision. */
async function writeProvenance(name, details) {
  const provenance = { ...source, capturedAt: new Date().toISOString(), ...details };
  await writeFile(resolve(out, `${name}.json`), `${JSON.stringify(provenance, null, 2)}\n`);
}

/**
 * Replay a recording from its start save in the player's own view, capturing
 * at 15 fps of recorded time (the headset's frame rate need not divide it).
 */
async function renderRecording(path, name) {
  const [header, ...frames] = (await readFile(path, "utf8")).trim().split("\n").map(JSON.parse);
  const debugFlags = ["--window-size", "1920x1080"];
  if (header.presentation === "Vr") debugFlags.push("--vr");
  if (header.experimental.length) debugFlags.push("--experimental", header.experimental.join(","));
  const game = await GameServer.launch({ mission: header.scene, debugFlags, repoRoot });
  const sink = await frameSink(game, name);
  try {
    await game.replay(path);
    await game.devParams.set("fov_override_deg", Number(values.fov));
    let clock = 0;
    let captured = 0;
    for (const frame of frames) {
      await game.step({ frames: 1 });
      clock += frame.dt;
      // A long frame spans several capture ticks: repeat the image for each.
      for (; clock >= captured / 15; captured++) await sink.capture();
    }
    await writeProvenance(name, {
      recording: basename(path),
      mission: header.scene,
      fovDeg: Number(values.fov),
      clip: sink.finish(),
    });
  } finally {
    await sink.dispose();
    await game.shutdown();
  }
}
