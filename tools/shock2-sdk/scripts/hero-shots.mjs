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
  handPoseAimedAt,
  quatMultiply,
  quatRotate,
  sampleTrack,
  setHandWorldPose,
  sway,
} from "../dist/src/index.js";
import { installWeaponUpgrade } from "../dist/test/helpers/weapon-upgrades.js";

// Hand offsets from the eye: x right, y up, -z toward the target.
const AIM_RIGHT = [0.1, -0.18, -0.55];
const AIM_LEFT = [-0.1, -0.2, -0.52];
const REST_LEFT = [-0.18, -0.35, -0.4];
const REST_RIGHT = [0.18, -0.4, -0.35];
// A rifle at low ready: the support hand sits under the handguard, in view.
const RIFLE_RIGHT = [0.18, -0.3, -0.3];
// A long gun shouldered, aimed down its length.
const SHOULDER_RIGHT = [0.16, -0.27, -0.4];
const RACK_RIGHT = [0.15, -0.31, -0.35];
// When command1's shotgun racks its pump, in clip seconds.
const RACKS = [0.45, 1.1, 1.75];
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
    hands: { right: RIFLE_RIGHT },
    // Installed, and so switched on, before the still.
    accessory: "Flashlight",
    twoHanded: "right",
    stats: { strength: 5, skills: { standard_weapons: 6 } },
    // Shots burst the pod; the clip ends before its gas reaches the player.
    // Its aim points jump as it bursts, so the aim stays where it stood.
    clip: {
      seconds: 0.95,
      hands: { right: [{ t: 0, value: RIFLE_RIGHT }] },
      trigger: { right: [0.3, 0.55, 0.8] },
      fixedAim: true,
      destroysSubject: true,
    },
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
    // Dual-wielded pistols.
    name: "rec1",
    mission: "rec1.mis",
    player: [-10.6, -2.9, -101.0],
    subject: "Red Monkey",
    // Creatures move slower on easy, so the monkey's charge stays in frame.
    difficulty: "easy",
    loadout: { right: "Pistol", left: "Pistol" },
    hands: { right: AIM_RIGHT, left: AIM_LEFT },
    // Raise both pistols from the hip and fire alternately, ending before the
    // charging monkey closes in under the sights. Each pistol's own shot
    // interval keeps a hand's pulls ~0.5 s apart.
    clip: {
      seconds: 1.05,
      everyPullFires: true,
      hands: {
        right: [
          { t: 0, value: REST_RIGHT },
          { t: 0.3, value: AIM_RIGHT },
        ],
        left: [
          { t: 0, value: REST_LEFT },
          { t: 0.35, value: AIM_LEFT },
        ],
      },
      trigger: { right: [0.35, 0.85], left: [0.5, 1.0] },
    },
  },
  {
    // Pump-action shotgun: fire, rack the pump with the support hand, fire.
    name: "command1",
    mission: "command1.mis",
    // The computer room, lit green by its terminals.
    player: [-325.6, -0.4, 81.1],
    // A suffix match: "Midwife" alone also matches the MidwifeGoto marker.
    subject: "*Midwife",
    loadout: { right: "Shotgun" },
    hands: { right: SHOULDER_RIGHT },
    twoHanded: "right",
    stats: { strength: 5, skills: { standard_weapons: 6 } },
    // Three shots, the pump racked between each: the pump alone paces it.
    clip: {
      seconds: 2.3,
      // The gun dips and draws back as each rack yanks the pump.
      hands: {
        right: RACKS.flatMap((t) => [
          { t, value: SHOULDER_RIGHT },
          { t: t + 0.17, value: RACK_RIGHT },
          { t: t + 0.4, value: SHOULDER_RIGHT },
        ]),
      },
      trigger: { right: [0.35, 0.95, 1.6] },
      // Each pull fires only if the pump was racked since the last.
      everyPullFires: true,
      pump: RACKS.flatMap((t) => [
        { t, value: 0 },
        { t: t + 0.17, value: 1 },
        { t: t + 0.35, value: 0 },
      ]),
    },
  },
  {
    // Reload: shoot the pistol dry, draw a clip from the belt pouch, seat it,
    // shoot again. A protocol droid walks slowly enough to wait for it.
    name: "ops3",
    mission: "ops3.mis",
    player: [23.5, -7.8, 149.8],
    subject: "Protocol Droid",
    loadout: { right: "Pistol" },
    hands: { right: AIM_RIGHT, left: REST_LEFT },
    // An untrained pistol will not fire.
    stats: { skills: { standard_weapons: 3 } },
    // On show on the chest while the eyes are down (chest slots take only
    // hypos and batteries): a med hypo and a psi hypo.
    chest: [-52, -57],
    reload: {
      spare: -31, // A standard clip, offered by the pouch.
      emptyAt: [22.6, -7.8, 131.6], // The mission start, out of the droid's earshot.
      shots: [0.15, 0.7], // Pulls, in seconds, before it runs dry and after the reload.
      glance: 0.75, // How far the eyes drop from the subject toward the pouch...
      gunGlance: 0.7, // ...and then onto the magazine well.
      tilt: 3, // Metres the pistol's aim lifts above the subject while the eyes are down...
      roll: -45, // ...and degrees the wrist rolls about the barrel (negative: clockwise).
      low: [0.12, -0.38, -0.4], // The pistol at low ready while the hand draws.
      seat: [-0.02, -0.32, -0.48], // Pistol held out in front to take the clip.
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
const repoRoot = process.env.SHOCK2_CAPTURE_REPO_ROOT
  ?? resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = values.out ? resolve(values.out) : resolve(repoRoot, "screenshots/hero");
await mkdir(out, { recursive: true });
// The source revision, read before captures overwrite tracked media.
const git = (...args) => execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
const source = {
  revision: process.env.SHOCK2_CAPTURE_REVISION ?? git("rev-parse", "HEAD"),
  dirty: git("status", "--porcelain", "--untracked-files=no") !== "",
  runtimeSha256: process.env.SHOCK2_CAPTURE_RUNTIME_SHA256,
};

if (values.replay) {
  await renderRecording(resolve(values.replay), values.name);
  process.exit(0);
}

const shots = values.only ? SHOTS.filter((s) => s.name === values.only) : SHOTS;
if (shots.length === 0) throw new Error(`no shot named '${values.only}'`);

// faceTarget steers continuously. Isolate capture preferences from the user's
// default snap turning and comfort mask, without changing their saved settings.
const captureSettings = { vr: { turning: "Smooth", vignette: "Off" } };
const settingsDir = await mkdtemp(resolve(tmpdir(), "hero-settings-"));
process.env.SHOCK2_SETTINGS_PATH = resolve(settingsDir, "user-settings.json");
await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify(captureSettings));
try {
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
} finally {
  await rm(settingsDir, { recursive: true, force: true });
}

async function captureShot(shot) {
  const game = await GameServer.launch({
    mission: shot.mission,
    difficulty: shot.difficulty,
    debugFlags: ["--vr", "--window-size", "1920x1080"],
    repoRoot,
  });
  try {
    // Turning arms after a neutral stick frame on a fresh settings load.
    await game.step({ frames: 1 });
    // Invulnerable, so a hit's red vignette never lands in the footage.
    await game.devParams.set("cheat", 1);
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
    if (shot.chest) await stowInChest(game, shot.chest);
    // Aim where the subject is now; it may have moved while we squared up.
    const points = await aimPoints(game, subject.id);
    assert.ok(points, `${shot.name}: the subject vanished while squaring up`);
    const target = points("torso");
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
    if (shot.accessory) await installAccessory(game, held.right, shot.accessory);
    if (shot.twoHanded) await attachSupportHand(game, shot.twoHanded);

    await game.devParams.set("fov_override_deg", Number(values.fov));
    await game.step({ frames: 2 });
    if (shot.accessory === "Flashlight") {
      const { carried_lights: lights = [] } = await game.scene.objects({ limit: 0 });
      assert.ok(lights.some((l) => l.kind === "spot"), `${shot.name}: the flashlight must light the scene`);
    }
    const path = resolve(out, `${shot.name}.png`);
    const result = await game.screenshot(path, Number(values["max-width"]));
    console.log(`${shot.name}: ${result.full_path} ${result.resolution.join("x")}`);
    const provenance = {
      mission: shot.mission,
      difficulty: shot.difficulty,
      settings: captureSettings,
      lighting: (await game.devParams.list()).params
        .filter((param) => ["ambient_light_intensity", "held_light_floor"].includes(param.key))
        .map(({ key, value, default: defaultValue }) => ({ key, value, default: defaultValue })),
      player: shot.player,
      subject: shot.subject ?? `template ${shot.spawnMonster}`,
      loadout: shot.loadout,
      accessory: shot.accessory,
      stats: shot.stats,
      fovDeg: Number(values.fov),
      still: { file: `${shot.name}.png`, resolution: result.resolution },
    };
    if (shot.clip) provenance.clip = await recordClip(game, shot, subject.id, held);
    if (shot.melee) provenance.melee = await recordMelee(game, shot, subject.id, held);
    if (shot.reload) provenance.reload = await recordReload(game, shot, subject.id, held);
    await writeProvenance(shot.name, provenance);
  } finally {
    await game.shutdown();
  }
}

/** Look up the subject's live aim points by class, falling back to the torso,
 * then its origin (which can sit well off its body - a monkey's is above its
 * back). Null once the subject is gone. */
async function aimPoints(game, id) {
  const detail = await game.entities.detail(id);
  if (!detail) return null;
  const find = (c) => detail.aim_points?.find((p) => p.classification === c)?.position;
  return (classification) => find(classification) ?? find("torso") ?? detail.position;
}

/**
 * Play `shot.clip` one 60 Hz frame at a time - eased hand tracks plus seeded
 * sway, aimed at the (moving) subject, trigger pulls at the listed times -
 * capturing every 4th frame (15 fps) into `<name>.gif`.
 */
async function recordClip(game, shot, subjectId, held) {
  const { clip } = shot;
  const frames = Math.round(clip.seconds * 60);
  const sink = await frameSink(game, shot.name);
  // Rounds left in each pulling hand's gun, for `everyPullFires`.
  const ammo = async () => {
    const rounds = {};
    for (const hand of Object.keys(clip.trigger)) rounds[hand] = await ammoOf(game, held[hand]);
    return rounds;
  };
  const ammoBefore = clip.everyPullFires ? await ammo() : undefined;
  let point;
  try {
    for (let frame = 0; frame < frames; frame++) {
      const t = frame / 60;
      const hands = Object.fromEntries(
        Object.entries(clip.hands).map(([hand, keys]) => [hand, sampleTrack(keys, t)]),
      );
      // A destroyed subject keeps its last aim.
      if (!clip.fixedAim || !point) point = (await aimPoints(game, subjectId)) ?? point;
      await trackSubject(game, point, t, hands);
      if (clip.pump) await rackPump(game, shot.twoHanded, sampleTrack(clip.pump, t));
      for (const [hand, pulls] of Object.entries(clip.trigger ?? {})) {
        const pulled = pulls.some((at) => t >= at && t < at + 0.12);
        await game.input.set(`${hand}_hand.trigger`, pulled ? 1 : 0);
      }
      await sink.step();
    }
    // Checked, so a take that went differently fails rather than misleads.
    if (shot.twoHanded) {
      const { player } = await game.info();
      const grip = player.hand_grips.find((g) => g.hand === shot.twoHanded);
      assert.ok(grip?.support?.attached, `${shot.name}: the support hand let go`);
    }
    if (clip.destroysSubject) {
      // A burst pod lingers as scenery, without hit points.
      const detail = await game.entities.detail(subjectId);
      const hp = Number(detail?.properties.find((p) => p.name === "HitPoints")?.value ?? 0);
      assert.ok(hp <= 0, `${shot.name}: the subject survived (${hp} hp)`);
    }
    if (clip.everyPullFires) {
      const ammoAfter = await ammo();
      for (const [hand, pulls] of Object.entries(clip.trigger)) {
        const fired = ammoBefore[hand] - ammoAfter[hand];
        assert.equal(fired, pulls.length, `${shot.name}: every ${hand} pull must fire`);
      }
    }
    return sink.finish();
  } finally {
    await sink.dispose();
  }
}

/** Install `accessory` on gun `id` through the real device chooser, which
 * switches it on, and let its "Installed ..." HUD line (5 s) expire. */
async function installAccessory(game, id, accessory) {
  await installWeaponUpgrade(game, id, accessory);
  await game.step({ frames: 310 });
}

/** Rounds loaded in gun `id`. A gun without ammo is a broken setup, not a bad take. */
async function ammoOf(game, id) {
  const ammo = (await game.entities.detail(id)).properties.find((p) => p.name === "Ammo");
  if (!ammo) throw new Error(`entity ${id} has no Ammo`);
  return Number(ammo.value);
}

/** Slide the support hand along the pump's rail to `fraction` (0 forward, 1 racked). */
async function rackPump(game, primary, fraction) {
  const { player } = await game.info();
  const socket = player.hand_grips.find((g) => g.hand === primary)?.support;
  assert.ok(socket?.attached && socket.pump, "the support hand must hold the pump");
  const { controller_position: p, controller_rotation: q } = socket;
  const delta = fraction - socket.pump.fraction;
  const travel = socket.pump.world_travel;
  await setHandWorldPose(
    game,
    player,
    primary === "left" ? "right" : "left",
    [p.x + travel.x * delta, p.y + travel.y * delta, p.z + travel.z * delta],
    [q.v.x, q.v.y, q.v.z, q.s],
  );
}

/** Stow one item per template in the chest slots, in order, with the right hand. */
async function stowInChest(game, templates) {
  for (const [slot, template] of templates.entries()) {
    // Clear of the chest first: a squeeze there would draw the last item back.
    await game.input.set("right_hand.position", [0.3, 1, -0.6]);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 1);
    const item = await game.player.spawnItem(template, { hand: "right" });
    await game.step({ frames: 3 });
    const { player } = await game.info();
    const center = player.hand_feedback?.chest_slots?.centers?.[slot];
    if (!center) throw new Error(`no chest slot ${slot}`);
    const [x, y, z, w] = player.rotation;
    const local = quatRotate([-x, -y, -z, w], center.map((v, i) => v - player.position[i]));
    await game.input.set("right_hand.position", local);
    await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 8 });
    const stowed = (await game.info()).player.hand_feedback?.chest_slots?.items?.[slot];
    if (stowed !== item.entity_id) throw new Error(`template ${template} did not stow in chest slot ${slot}`);
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
      point = (await aimPoints(game, subjectId)) ?? point;
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
      await game.input.set("left_hand.thumbstick", [0, 0.6]);
      await hold(1 / 60, shot.hands);
    }
    await game.input.set("left_hand.thumbstick", [0, 0]);
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
      point = (await aimPoints(game, subjectId)) ?? point;
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
 * Shoot the pistol dry, draw a clip from the belt pouch with the free left
 * hand, seat it in the magazine well and shoot again. The pouch reach and the
 * insert are closed loops on the palm and the clip's own position, so no grip
 * offset is assumed; the insert and both volleys are asserted.
 */
async function recordReload(game, shot, subjectId, held) {
  const { reload } = shot;
  const rounds = () => ammoOf(game, held.right);
  const pull = async (pulled) => game.input.set("right_hand.trigger", pulled ? 1 : 0);
  let point = await aimPoints(game, subjectId);

  // Off camera: a spare clip into the pack for the pouch to offer, then the
  // magazine emptied into the ceiling down to its last rounds - from
  // `emptyAt`, beyond the subject's hearing, so it is not drawn in.
  await game.player.spawnItem(reload.spare);
  const [ex, ey, ez] = reload.emptyAt;
  await game.player.teleport({ x: ex, y: ey, z: ez });
  await game.step({ frames: 5 });
  for (let pulls = 0; (await rounds()) > reload.shots.length; pulls++) {
    assert.ok(pulls < 40, `${shot.name}: the pistol would not empty`);
    const { player } = await game.info();
    const ceiling = player.position.map((v, i) => v + (i === 1 ? 10 : 0));
    await aimHandsAt(game, point("head"), { right: shot.hands.right }, ceiling);
    await pull(true);
    await game.step({ frames: 2 });
    await pull(false);
    await game.step({ frames: 40 });
  }
  const [x, y, z] = shot.player;
  await game.player.teleport({ x, y, z });
  await game.step({ frames: 5 });
  point = (await aimPoints(game, subjectId)) ?? point;

  const sink = await frameSink(game, shot.name);
  let t = 0;
  const eyeOf = (player) => player.position.map((v, i) => v + (i === 1 ? player.camera_offset[1] : 0));
  // `world` as an offset from the eye in the yaw frame of a view toward `look`
  // (what `handPoseAimedAt` takes), and back.
  const yawTo = (eye, look) => Math.atan2(-(look[0] - eye[0]), -(look[2] - eye[2]));
  const rotateY = (v, a) => [v[0] * Math.cos(a) + v[2] * Math.sin(a), v[1], -v[0] * Math.sin(a) + v[2] * Math.cos(a)];
  const toOffset = (eye, look, world) => rotateY(world.map((v, i) => v - eye[i]), -yawTo(eye, look));
  const fromOffset = (eye, look, offset) => rotateY(offset, yawTo(eye, look)).map((v, i) => v + eye[i]);
  const lerp = (a, b, s) => a.map((v, i) => v + (b[i] - v) * s);
  const ease = (s) => s * s * (3 - 2 * s);
  // A look point a fraction `g` of the way from looking at `a` to looking at `b`.
  const glance = (eye, a, b, g) => {
    const unit = (p) => {
      const d = p.map((v, i) => v - eye[i]);
      return d.map((v) => v / Math.hypot(...d));
    };
    return lerp(unit(a), unit(b), g).map((v, i) => eye[i] + v * 5);
  };
  // One frame: eyes on `look`, each hand at its eye-frame offset, aimed at
  // `aims[hand]`. Tracks where the left hand was sent, in world space.
  // Offsets are in the yaw frame facing `facing`, not the look: looking almost
  // straight down, the look's yaw is unstable, and hands placed in it would
  // swing the gun around - and the look that follows the gun with it.
  let leftWorld;
  let facing;
  // `tilt` (0..1) eases in the pistol's off-subject pose: muzzle lifted by
  // `reload.tilt` and the wrist rolled by `reload.roll` about the barrel.
  const pose = async (look, offsets, aims, tilt = 0) => {
    const { player } = await game.info();
    await game.input.lookAtWorldPoint(look);
    if (offsets.left) leftWorld = fromOffset(eyeOf(player), facing, offsets.left);
    for (const [hand, offset] of Object.entries(offsets)) {
      const p = handPoseAimedAt(player.position, player.rotation, player.camera_offset[1], facing, offset, aims[hand]);
      const roll = hand === "right" ? (reload.roll * tilt * Math.PI) / 360 : 0;
      await game.input.set(`${hand}_hand.position`, p.position);
      await game.input.set(`${hand}_hand.rotation`, quatMultiply(p.rotation, [0, 0, Math.sin(roll), Math.cos(roll)]));
    }
    await sink.step();
    t += 1 / 60;
  };
  // Hold on the subject for `seconds`, pulling the trigger at `pulls` (clip-relative).
  const engage = async (seconds, pulls = []) => {
    for (const end = t + seconds; t < end; ) {
      point = (await aimPoints(game, subjectId)) ?? point;
      await trackSubject(game, point, t, { right: shot.hands.right, left: shot.hands.left });
      await pull(pulls.some((at) => t >= at && t < at + 0.12));
      await sink.step();
      t += 1 / 60;
    }
    await pull(false);
  };

  try {
    // 1. The last rounds.
    await engage(0.9, reload.shots);
    assert.equal(await rounds(), 0, `${shot.name}: the pistol must run dry`);

    // 2. Glance down; the left hand drops to the pouch, the gun to low ready.
    const draw = 54;
    // The belt follows the head, so the pouch is re-read every frame.
    const pouchOf = (player) => {
      const center = player.hand_feedback?.ammo_pouch?.center;
      if (!center) throw new Error(`${shot.name}: no ammo pouch`);
      return center;
    };
    const head = point("head");
    facing = head;
    // Muzzle up while the eyes are down, so the locked-back slide shows side-on.
    const tilted = (s) => point("torso").map((v, i) => v + (i === 1 ? reload.tilt * s : 0));
    let leftOffset = shot.hands.left;
    let pouch;
    for (let f = 1; f <= draw; f++) {
      const s = ease(f / draw);
      const { player } = await game.info();
      const eye = eyeOf(player);
      pouch = pouchOf(player);
      const look = glance(eye, head, pouch, s * reload.glance);
      // The pouch takes the palm, not the controller origin: aim the hand so
      // its palm lands there.
      const palm = player.hand_feedback?.glove_contacts?.centers[0];
      const palmFromHand = palm && leftWorld ? palm.map((v, i) => v - leftWorld[i]) : [0, 0, 0];
      const want = toOffset(eye, facing, pouch.map((v, i) => v - palmFromHand[i]));
      leftOffset = lerp(shot.hands.left, want, s);
      await pose(look, { left: leftOffset, right: lerp(shot.hands.right, reload.low, s) }, {
        left: lerp(eye, pouch, 2), right: tilted(s),
      }, s);
    }
    await game.input.set("left_hand.squeeze", 1);
    for (let f = 0; f < 12; f++) {
      const { player } = await game.info();
      pouch = pouchOf(player);
      const look = glance(eyeOf(player), head, pouch, reload.glance);
      await pose(look, { left: leftOffset, right: reload.low }, { left: lerp(eyeOf(player), pouch, 2), right: tilted(1) }, 1);
    }
    const clip = (await game.info()).player.wielded_entity_id;
    assert.ok(clip != null, `${shot.name}: the pouch must put a clip in the left hand`);

    // 3. Carry the clip up under the pistol, then push it slowly up into the
    // well. The well takes a clip within 0.25 of its anchor, so the eased
    // approach stops outside that and only the slow push enters it.
    const approach = 66;
    const push = 36;
    const from = (await game.entities.detail(clip)).position;
    let look;
    const seat = async (f) => {
      const s = ease(Math.min(f / approach, 1));
      const [{ player }, gun, mag] = await Promise.all([
        game.info(), game.entities.detail(held.right), game.entities.detail(clip),
      ]);
      const eye = eyeOf(player);
      const anchor = gun.magazine_anchor;
      const below = anchor.map((v, i) => v - (i === 1 ? 0.4 : 0));
      const want = f <= approach
        ? lerp(from, below, s)
        : lerp(below, anchor, Math.min((f - approach) / push, 1));
      look = glance(eye, glance(eye, head, pouch, reload.glance), anchor, s * reload.gunGlance);
      // Move the hand by the clip's error from where it should be. A seated
      // clip is gone, so the hand then holds where it is.
      if (mag) leftOffset = toOffset(eye, facing, leftWorld.map((v, i) => v + want[i] - mag.position[i]));
      await pose(look, { left: leftOffset, right: lerp(reload.low, reload.seat, s) }, {
        // Up and forward in the view's frame, so the clip's yaw stays put
        // whatever the loop is correcting.
        left: fromOffset(eye, facing, leftOffset.map((v, i) => v + [0, 1, -0.6][i])), right: tilted(1),
      }, 1);
    };
    let f = 1;
    for (; f <= approach + push && (await rounds()) === 0; f++) await seat(f);
    // Hold the seated gun a beat before letting go.
    for (const end = f + 12; f < end; f++) await seat(f);
    const loaded = await rounds();
    assert.ok(loaded > 0, `${shot.name}: the clip must seat`);

    // 4. Let go, bring both hands back up on the subject.
    await game.input.set("left_hand.squeeze", 0);
    const recover = 36;
    const lastLeft = leftOffset;
    for (let f = 1; f <= recover; f++) {
      const s = ease(f / recover);
      point = (await aimPoints(game, subjectId)) ?? point;
      const { player } = await game.info();
      const eye = eyeOf(player);
      const back = glance(eye, look, point("head"), s);
      await pose(back, { left: lerp(lastLeft, shot.hands.left, s), right: lerp(reload.seat, shot.hands.right, s) }, {
        left: point("torso"), right: tilted(1 - s),
      }, 1 - s);
    }

    // 5. Back in the fight.
    const resumed = t;
    await engage(1.0, reload.shots.map((at) => resumed + at));
    assert.equal(await rounds(), loaded - reload.shots.length, `${shot.name}: every shot after the reload must fire`);
    return { ...sink.finish(), loaded };
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
