// Reproducible README/website hero stills. Built SDK; run from tools/shock2-sdk:
//   npm run build && node scripts/hero-shots.mjs [--only hydro1] [--out <dir>]
//   node scripts/hero-shots.mjs --replay <rec-*.jsonl> --name climb   # a recorded session as a GIF
// Each shot boots its mission fresh in VR presentation (no screen-space HUD),
// stands the player at `player` with a loadout in hand, aimed at the nearest
// entity matching `subject` - a first-person VR view - and, for shots with a
// `clip`, records it as a GIF. AI wanders, so framing is only coarsely
// reproducible.
import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  aimHandsAt,
  attachSupportHand,
  faceTarget,
  GameServer,
  sampleTrack,
  sway,
} from "../dist/src/index.js";

// Hand offsets from the eye: x right, y up, -z toward the target.
const AIM_RIGHT = [0.1, -0.18, -0.55];
const AIM_LEFT = [-0.1, -0.2, -0.52];
const REST_LEFT = [-0.18, -0.35, -0.4];
const REST_RIGHT = [0.18, -0.4, -0.35];

const SHOTS = [
  {
    // Two-handed long gun.
    name: "hydro1",
    mission: "hydro1.mis",
    player: [38.3, 0.9, -16.4],
    subject: "OG-Shotgun",
    loadout: { right: "Shotgun" },
    hands: { right: [0.12, -0.2, -0.36] },
    twoHanded: "right",
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
    "gif-width": { type: "string", default: "480" },
    replay: { type: "string" },
    name: { type: "string", default: "replay" },
  },
});
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = values.out ? resolve(values.out) : resolve(repoRoot, "screenshots/hero");
await mkdir(out, { recursive: true });

if (values.replay) {
  await renderRecording(resolve(values.replay), values.name);
  process.exit(0);
}

const shots = values.only ? SHOTS.filter((s) => s.name === values.only) : SHOTS;
if (shots.length === 0) throw new Error(`no shot named '${values.only}'`);

for (const shot of shots) {
  const game = await GameServer.launch({
    mission: shot.mission,
    debugFlags: ["--vr", "--window-size", "1920x1080"],
    repoRoot,
  });
  try {
    if (shot.stats) await game.player.setStats(shot.stats);
    const { entities } = await game.entities.list({ filter: shot.subject, limit: 50 });
    const dist = (e) => Math.hypot(...e.position.map((v, i) => v - shot.player[i]));
    const subject = entities.sort((a, b) => dist(a) - dist(b))[0];
    if (!subject) throw new Error(`${shot.name}: no '${shot.subject}' in ${shot.mission}`);
    const heading = subject.position.map((v, i) => v - shot.player[i]);

    // Square up at the spawn point, out of the subject's sight, along the
    // shot's heading (settling takes seconds - long enough to draw an attack).
    const spawn = (await game.info()).player.position;
    await faceTarget(game, spawn.map((v, i) => v + heading[i]));
    const [x, y, z] = shot.player;
    await game.player.teleport({ x, y, z });
    await game.step({ frames: 5 });
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
    if (shot.clip) await recordClip(game, shot, subject.id);
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
  const dir = await mkdtemp(resolve(tmpdir(), `hero-${shot.name}-`));
  try {
    for (let frame = 0; frame < frames; frame++) {
      const t = frame / 60;
      // Track the subject: eyes on its head (a charging creature's torso would
      // pitch the view into the floor), hands on its torso, both drifting.
      const drift = sway(1, t, 0.04);
      const point = await aimPoints(game, subjectId);
      const look = point("head").map((v, i) => v + drift[i]);
      const body = point("torso").map((v, i) => v + drift[i]);
      const hands = Object.fromEntries(
        Object.entries(clip.hands).map(([hand, keys]) => {
          const tremor = sway(hand === "right" ? 2 : 3, t, 0.008, 0.6);
          return [hand, sampleTrack(keys, t).map((v, i) => v + tremor[i])];
        }),
      );
      await aimHandsAt(game, look, hands, body);
      for (const [hand, pulls] of Object.entries(clip.trigger ?? {})) {
        const pulled = pulls.some((at) => t >= at && t < at + 0.12);
        await game.input.set(`${hand}_hand.trigger`, pulled ? 1 : 0);
      }
      await game.step({ frames: 1 });
      if (frame % 4 === 0) {
        const name = String(frame / 4).padStart(4, "0");
        await game.screenshot(resolve(dir, `${name}.png`), Number(values["gif-width"]));
      }
    }
    writeGif(dir, shot.name);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

/** Assemble `dir/0000.png...` (15 fps) into `<out>/<name>.gif`. */
function writeGif(dir, name) {
  const gif = resolve(out, `${name}.gif`);
  execFileSync("ffmpeg", [
    "-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(dir, "%04d.png"),
    "-vf", "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    gif,
  ]);
  console.log(`${name}: ${gif}`);
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
  const dir = await mkdtemp(resolve(tmpdir(), `hero-${name}-`));
  try {
    await game.replay(path);
    await game.devParams.set("fov_override_deg", Number(values.fov));
    let clock = 0;
    let captured = 0;
    for (const frame of frames) {
      await game.step({ frames: 1 });
      clock += frame.dt;
      if (clock >= captured / 15) {
        const file = resolve(dir, `${String(captured).padStart(4, "0")}.png`);
        await game.screenshot(file, Number(values["gif-width"]));
        captured++;
      }
    }
    writeGif(dir, name);
  } finally {
    await rm(dir, { recursive: true, force: true });
    await game.shutdown();
  }
}
