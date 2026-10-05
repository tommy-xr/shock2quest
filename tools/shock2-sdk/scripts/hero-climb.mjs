// Capture a real VR ladder climb and mantle in medsci1 for the website hero:
// overhand grips on the ladder model's rungs, rung over rung, onto the deck.
// Run from tools/shock2-sdk after npm run build:
// DARK_ASSET_PATH=/path/to/25AE node scripts/hero-climb.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  GameServer, ladderHoldPoints, lookQuat, quatMultiply, quatRotate, recordClipInputs,
  viewRight, vrGrab, vrReach,
} from "../dist/src/index.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const framesDir = await mkdtemp(resolve(tmpdir(), "hero-climb-"));
const samplesDir = resolve(tmpdir(), "hero-climb-samples");
// A SIDE_VIEWS run steps outside the clip, so it never replaces the committed take.
const out = process.env.SIDE_VIEWS ? samplesDir : resolve(root, "screenshots/hero");
const assetRoot = process.env.DARK_ASSET_PATH;
assert.ok(assetRoot, "set DARK_ASSET_PATH to the 25AE remaster asset root");
for (const file of ["sshock2.kpf", ...["400", "patch_ext", "scp", "shtup", "sshock2ee"].map((name) => `mods/${name}.kpf`)]) {
  assert.ok((await stat(resolve(assetRoot, file))).size > 0, `missing ${file} from 25AE remaster assets`);
}
await mkdir(out, { recursive: true });
await mkdir(samplesDir, { recursive: true });

// The medsci1 shaft ladder: a stack of `rickladd` segments facing +z.
const COLUMN = [-17.54, -4, 14.5];
const STAGE = { x: -17.54, y: -4.5, z: 14.65 };
// Engine lighting controls only; geometry and materials stay production.
const LIGHTING = { ambient_light_intensity: 1.6, level_light_intensity: 2.0, held_light_floor: 1.0 };
// Overhand grip: the hand points this far above level into the ladder, palm
// rolled down onto the bar.
const RUNG_PITCH = 45;
// The grip point sits this far above each rung and toward the climber, so the
// fingers close over the bar instead of through it. Well inside the runtime's
// 0.2 hold reach, so the grip is still a ladder hold.
const RUNG_GRIP_UP = 0.06;
const RUNG_GRIP_OUT = 0.03;
// On the deck the fingers point forward, away from the climber, turned in only
// slightly, the wrist a little above the fingers as when pressing to vault.
const DECK_PITCH = -10;
const DECK_TOE_IN = 10;
// Before the mantle, a last part-pull on the top rung lifts the eye over the
// deck (a pull of a whole rung would take the hands past arm's reach), so it
// looks down on the backs of the planted hands, not into their open cuffs.
const MANTLE_RISE = 0.25;
// Tracked head distance off the ladder face while climbing.
const EYE_GAP = 0.5;
// While planting on the deck the head leans in to the ladder face, nearer over the hands.
const MANTLE_EYE_GAP = 0;
// Aim this far above the hands, so the rung they close on sits mid-frame.
const GAZE_ABOVE_HANDS = 0.05;
const REACH_FRAMES = 20;
const PULL_FRAMES = 32;
const PRESS_FRAMES = 80;
const LANDED_FRAMES = 8;
const FPS = 15;
const SIM_PER_SHOT = 60 / FPS;

const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const add = (a, b) => a.map((c, i) => c + b[i]);
const scale = (a, s) => a.map((c) => c * s);
const lerp = (a, b, t) => add(a, scale(add(b, scale(a, -1)), t));
const axisQuat = (axis, deg) => {
  const half = (deg * Math.PI) / 360;
  return [...scale(axis, Math.sin(half)), Math.cos(half)];
};
/** Normalized lerp between rotations, along the shorter arc. */
const nlerp = (a, b, t) => {
  const q = lerp(a, a.reduce((d, c, i) => d + c * b[i], 0) < 0 ? scale(b, -1) : b, t);
  return scale(q, 1 / Math.hypot(...q));
};

/**
 * World rotation of an overhand grip: the controller's -Z (the glove's
 * knuckle direction) points `into` the ladder, raised `pitch` degrees, and
 * the palm is rolled to face down - knuckles up, fingers curled over.
 */
function overhand(hand, into, pitch) {
  const p = (pitch * Math.PI) / 180;
  const aim = add(scale(into, Math.cos(p)), [0, Math.sin(p), 0]);
  return quatMultiply(lookQuat(aim), axisQuat([0, 0, 1], hand === "right" ? 90 : -90));
}

// Each hand's last world rotation, for turning it smoothly from there.
const handRotation = {};
/** Set `hand`'s controller rotation (pawn-local) from a world rotation. */
async function setHandRotation(game, hand, world) {
  handRotation[hand] = world;
  const [x, y, z, w] = (await game.info()).player.rotation;
  await game.input.set(`${hand}_hand.rotation`, quatMultiply([-x, -y, -z, w], world));
}

/** Every ladder segment the column through `near` crosses, from its model. */
async function laddersOnColumn(game, near, fromY, toY) {
  const ids = new Set();
  for (let y = fromY; y <= toY; y += 0.2) {
    const grip = (await game.physics.grip([near[0], y, near[2]])).grip;
    if (grip?.kind === "ladder" && grip.entity_id !== null) ids.add(grip.entity_id);
  }
  return Promise.all([...ids].map((id) => game.physics.ladder(id)));
}

const game = await GameServer.launch({
  mission: "medsci1.mis",
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});

let tick = 0;
let captured = 0;
let phase = "stage";
const samples = [];
const look = { target: null, hands: false, deck: null };
async function shot() {
  const path = resolve(framesDir, `${String(captured).padStart(4, "0")}.png`);
  await game.screenshot(path, 1280);
  const { player } = await game.info();
  samples.push({ frame: tick, image: path, phase, position: player.position,
    grips: player.climb.grips.map(({ hand, kind }) => ({ hand, kind })),
    vaulting: player.climb.vaulting, health: player.hit_points });
  captured++;
}
try {
  await game.step({ frames: 5 });
  const { params } = await game.devParams.list();
  assert.ok(params.some((p) => p.key === "cheat"), "runtime lacks the cheat dev param: stale binary");
  // Invulnerable: a creature below otherwise floods the clip with damage.
  await game.devParams.set("cheat", 1);
  for (const [key, value] of Object.entries(LIGHTING)) await game.devParams.set(key, value);
  await game.player.teleport(STAGE);
  await game.step({ frames: 10 });
  await game.input.lookAtWorldPoint([COLUMN[0], -1.5, 14.4]);

  const { player: start } = await game.info();
  const ladders = await laddersOnColumn(game, COLUMN, start.position[1] - 2, start.position[1] + 5);
  assert.ok(ladders.length >= 3, `expected the stacked shaft ladder, found ${ladders.length} segments`);
  const toClimber = ladders[0].normal;
  // A point on the ladder face (its rails), for distances off it.
  const face = ladders[0].rails[0][0];
  const into = scale(toClimber, -1);
  for (const hand of ["left", "right"]) await setHandRotation(game, hand, overhand(hand, into, RUNG_PITCH));

  // Record from before staging so grips held into the clip start pressed.
  const inputs = recordClipInputs(game, { fps: FPS });
  // Staging, outside the clip: both hands on the highest rung below the eye.
  const right = await viewRight(game);
  const holds = Object.fromEntries(["left", "right"].map((hand) => [hand, ladders
    .flatMap((ladder) => ladderHoldPoints(ladder, hand, "rung", start.position, right))
    .map((p) => add(p, add([0, RUNG_GRIP_UP, 0], scale(toClimber, RUNG_GRIP_OUT))))
    .sort((a, b) => a[1] - b[1])]));
  const eyeY = start.position[1] + start.camera_offset[1];
  let rung = holds.right.findLastIndex((p) => p[1] <= eyeY);
  assert.ok(rung >= 0, "no rung below eye height");
  const top = holds.left.length - 1;
  // One pull lifts the body one rung.
  const pull = [0, -(holds.left[top][1] - holds.left[0][1]) / top, 0];
  for (const hand of ["right", "left"]) {
    assert.equal((await vrGrab(game, hand, holds[hand][rung])).kind, "ladder");
  }
  /** Move both gripping controllers by `delta` (pawn space) together; returns the body path. */
  async function pullBoth(delta, frames, until) {
    const state = await game.input.state();
    const start = { left: state.left_hand.position, right: state.right_hand.position };
    const path = [];
    for (let frame = 1; frame <= frames; frame++) {
      for (const hand of ["left", "right"]) {
        await game.input.set(`${hand}_hand.position`, add(start[hand], scale(delta, frame / frames)));
      }
      await game.step({ frames: 1 });
      const { player } = await game.info();
      path.push(player.position);
      if (until?.(player)) break;
    }
    return path;
  }
  // Let staging HUD lines (e.g. a shoulder-zone notice) expire on the ladder.
  await game.step({ frames: 330 });
  // A climber's head keeps a steady gap to the rungs while the collision
  // capsule shifts on and off them: hold the tracked head EYE_GAP off the
  // ladder face, and settle it back over the body once the vault starts.
  const pawn = (await game.info()).player;
  const inversePawn = [-pawn.rotation[0], -pawn.rotation[1], -pawn.rotation[2], pawn.rotation[3]];
  let lean = 0;
  let gap = EYE_GAP;
  const holdHead = async (player, settle) => {
    const offFace = (player.position[0] - face[0]) * toClimber[0] + (player.position[2] - face[2]) * toClimber[2];
    gap += ((phase === "mantle" ? MANTLE_EYE_GAP : EYE_GAP) - gap) * 0.1;
    lean = settle ? lean * 0.85 : gap - offFace;
    await game.input.set("head.position",
      add([0, pawn.camera_offset[1], 0], quatRotate(inversePawn, scale(toClimber, lean))));
  };
  await holdHead(pawn, false);

  const handWorld = async () => {
    const state = await game.input.state();
    return { left: state.left_hand.world_position, right: state.right_hand.world_position };
  };
  // The rendered eye: the runtime caps the tracked head's height (lower on a
  // ladder), so read it back rather than trusting the head we set.
  const eyeNow = async () => {
    const { player } = await game.info();
    return add(player.position, quatRotate(player.rotation, player.camera_offset));
  };
  // The point square ahead of the eye at `point`'s height and distance off
  // the ladder: pitch only, since a yaw while looking this steeply down
  // reads as a roll.
  const squareTo = (point, eye) => {
    const off = add(point, scale(eye, -1));
    return add(eye, add(scale(into, Math.abs(off[0] * toClimber[0] + off[2] * toClimber[2])), [0, off[1], 0]));
  };
  // Climbing, watch both hands; mantling, halfway between them and the deck,
  // so the hands on the rung and the deck they reach for share the frame.
  const handsGaze = async (eye) => {
    const hands = await handWorld();
    const watch = add(lerp(hands.left, hands.right, 0.5), [0, GAZE_ABOVE_HANDS, 0]);
    return squareTo(look.deck ? lerp(watch, look.deck, 0.5) : watch, eye);
  };
  // Ease the gaze between targets; aim at a far point along the eye's line,
  // since lookAtWorldPoint assumes an eye straight above the pawn.
  let gaze = null;
  const aimHead = async () => {
    const eye = await eyeNow();
    if (look.hands) look.target = await handsGaze(eye);
    gaze = gaze ? lerp(gaze, look.target, 0.5) : look.target;
    await game.input.lookAtWorldPoint(add(eye, scale(add(gaze, scale(eye, -1)), 40)));
  };

  const step = game.step.bind(game);
  game.step = async ({ frames = 1 } = {}) => {
    let advanced = 0;
    for (let i = 0; i < frames; i++) {
      const { player } = await game.info();
      await holdHead(player, player.climb.vaulting || phase === "landed" || phase === "done");
      await aimHead();
      advanced += (await step({ frames: 1 })).frames_advanced;
      tick++;
      // Video frame j is the screenshot after the clip's step 4j + 1.
      if ((tick - 1) % SIM_PER_SHOT === 0 && phase !== "done") await shot();
    }
    return { frames_advanced: advanced };
  };

  /**
   * Move `hand`'s grip point to `point` the way an arm does: an arc `arc` off
   * the face, turning the hand to world `rotation` on the way when given.
   */
  async function reachTo(hand, point, arc = 0.12, rotation = null) {
    const from = (await handWorld())[hand];
    const fromRotation = handRotation[hand];
    for (let f = 1; f <= REACH_FRAMES; f++) {
      const t = f / REACH_FRAMES;
      const eased = t * t * (3 - 2 * t);
      if (rotation) await setHandRotation(game, hand, nlerp(fromRotation, rotation, eased));
      await vrReach(game, hand, add(lerp(from, point, eased), scale(toClimber, arc * Math.sin(Math.PI * t))));
      await game.step({ frames: 1 });
    }
  }
  // Verification only (SIDE_VIEWS=1): photograph a fresh grip from beside the
  // ladder with the free camera. Steps outside the clip, so such a run is no take.
  async function sideView(name, hand, target) {
    if (!process.env.SIDE_VIEWS) return;
    const side = scale(right, hand === "right" ? 1 : -1);
    const views = { a: add(scale(side, -0.2), [0, 0.02, 0]),
      b: add(add(scale(side, 0.5), scale(toClimber, 0.35)), [0, 0.45, 0]),
      c: add(scale(side, 0.7), [0, 0.1, 0]) };
    for (const [view, offset] of Object.entries(views)) {
      await game.camera.set({ position: add(target, offset), lookAt: target });
      await step({ frames: 1 });
      await game.screenshot(resolve(samplesDir, `side-${name}-${view}.png`), 1280);
    }
    await game.camera.attach();
    await step({ frames: 1 });
  }
  async function release(hand) {
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.input.set(`${hand}_hand.world_target`, null);
  }

  phase = "climb";
  look.hands = true;
  look.target = await handsGaze(await eyeNow());
  inputs.start();
  const strokes = [];
  const reachTicks = [];
  // Rung over rung: both hands pull together, keeping both gloves on rungs
  // through the pull, then each in turn reaches the next rung up - just
  // below the eye, clear of the shoulder stow zones - and closes on it.
  for (;;) {
    const path = await pullBoth(pull, PULL_FRAMES);
    assert.equal((await game.info()).player.climb.grips.length, 2, "both hands hold through the pull");
    strokes.push({ rung, bodyY: path.at(-1)[1] });
    if (rung === top) break;
    for (const hand of ["left", "right"]) {
      await release(hand);
      reachTicks.push(tick);
      await reachTo(hand, holds[hand][rung + 1]);
      const held = await vrGrab(game, hand, holds[hand][rung + 1]);
      assert.equal(held.kind, "ladder", `the ${hand} hand must close on a rung`);
      await sideView(`rung${rung + 1}-${hand}`, hand, holds[hand][rung + 1]);
    }
    rung += 1;
  }
  assert.ok(strokes.length >= 2, `expected at least two strokes, got ${strokes.length}`);

  // Mantle: both hands over the lip onto the deck - the first ledge a probe
  // finds beyond the top segment's rails - then press down until the vault.
  phase = "mantle";
  const topY = Math.max(...ladders.flatMap((ladder) => ladder.rails.flat().map((p) => p[1])));
  const ledges = {};
  for (const hand of ["left", "right"]) {
    for (let d = 0.1; d <= 1.0 && !ledges[hand]; d += 0.05) {
      const probe = add([holds[hand][top][0], topY + 0.1, face[2]], scale(into, d));
      if ((await game.physics.grip(probe)).grip?.kind === "ledge") ledges[hand] = add(probe, scale(into, 0.15));
    }
    assert.ok(ledges[hand], `no deck ledge beyond the ladder top for the ${hand} hand`);
  }
  look.deck = lerp(ledges.left, ledges.right, 0.5);
  await pullBoth(scale(pull, MANTLE_RISE), PULL_FRAMES);
  for (const hand of ["left", "right"]) {
    await release(hand);
    const inward = scale(right, hand === "left" ? 1 : -1);
    const toe = (DECK_TOE_IN * Math.PI) / 180;
    const forward = add(scale(into, Math.cos(toe)), scale(inward, Math.sin(toe)));
    await reachTo(hand, ledges[hand], 0, overhand(hand, forward, DECK_PITCH));
    assert.equal((await vrGrab(game, hand, ledges[hand])).kind, "ledge", `the ${hand} hand must close on the deck`);
    await sideView(`deck-${hand}`, hand, ledges[hand]);
  }
  look.hands = false;
  const pressPath = await pullBoth([0, -1.2, 0], PRESS_FRAMES, (player) => player.climb.vaulting);
  assert.ok((await game.info()).player.climb.vaulting, "pressing down on the deck must start the vault");
  // The hands open and stay planted on the deck while the body rises over
  // them, until out of reach.
  for (const hand of ["left", "right"]) await game.input.set(`${hand}_hand.squeeze`, 0);
  const planted = { left: true, right: true };
  // Through the rise, watch the deck just past the planted hands; once they
  // lift off, look ahead down the deck.
  const onDeck = add(lerp(ledges.left, ledges.right, 0.5), scale(into, 0.5));
  const ahead = add(lerp(ledges.left, ledges.right, 0.5), scale(into, 3));
  let landedFrames = 0;
  let landed = null;
  for (let frame = 0; frame < 300 && landedFrames < LANDED_FRAMES + 12; frame++) {
    for (const hand of ["left", "right"]) {
      if (!planted[hand]) continue;
      try {
        await vrReach(game, hand, ledges[hand]);
      } catch (error) {
        if (!String(error.message).startsWith("vrReach:")) throw error;
        // Out of reach: the hand lifts off and rides along.
        planted[hand] = false;
        await game.input.set(`${hand}_hand.world_target`, null);
      }
    }
    const { player } = await game.info();
    if (!player.climb.vaulting) landedFrames++;
    look.target = planted.left || planted.right
      ? squareTo(onDeck, await eyeNow())
      : lerp(look.target, ahead, 0.15);
    // Hold the landing for a beat, then stop filming the settle.
    phase = player.climb.vaulting ? "mantle" : landedFrames <= LANDED_FRAMES ? "landed" : "done";
    await game.step({ frames: 1 });
    if (landedFrames > 0) landed = (await game.info()).player.position;
  }
  assert.ok(landed, "the vault must land");
  const after = (await game.info()).player;
  const beyond = landed.reduce((sum, c, i) => sum + (c - face[i]) * into[i], 0);
  assert.ok(beyond > 0.3, `the mantle must land past the ladder face (${beyond.toFixed(2)})`);
  assert.equal(after.climb.grips.length, 0, "grips released after the mantle");
  assert.ok(samples[0].health > 0 && samples.every((s) => s.health === samples[0].health), "no damage during the clip");
  assert.ok(captured >= 75 && captured <= 90, `expected a 5-6 second clip, got ${captured} frames`);
  // The timeline ends with the video, not with the settle filmed past it.
  await writeFile(resolve(out, "climb.inputs.json"),
    `${JSON.stringify({ ...inputs.timeline(), duration: captured / FPS })}\n`);
  inputs.dispose();

  const input = resolve(framesDir, "%04d.png");
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(FPS), "-i", input,
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an",
    "-movflags", "+faststart", resolve(out, "climb.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", String(FPS), "-i", input,
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "climb.gif")]);
  // Poster: both fists closed on a rung as the pull starts.
  const named = [
    ["climb.png", samples.find((s) => s.frame > reachTicks[1] + REACH_FRAMES + 4 && s.grips.length === 2)],
    ["climb-pull.png", samples.findLast((s) => s.frame < reachTicks.at(-1) && s.grips.length === 2)],
    ["climb-deck.png", samples.findLast((s) => s.phase === "mantle" && !s.vaulting && s.grips.length === 2)],
    ["climb-mantle.png", samples.find((s) => s.vaulting)],
    ["climb-landed.png", samples.findLast((s) => s.phase === "landed")],
  ];
  for (const [name, sample] of named) {
    assert.ok(sample, `missing ${name} phase`);
    await copyFile(sample.image, name === "climb.png" ? resolve(out, name) : resolve(samplesDir, name));
  }
  const runtime = resolve(root, process.env.CARGO_TARGET_DIR ?? "target", "debug/debug_runtime");
  await writeFile(resolve(out, "climb.json"), JSON.stringify({
    mission: "medsci1.mis", presentation: "Vr", revision: git("rev-parse", "HEAD"),
    // A capture from a local merge of unmerged branches lists them here.
    revisionParents: git("log", "-1", "--format=%P").split(" "),
    runtimeSha256: await sha256(runtime), assetRoot,
    devParams: { cheat: 1, ...LIGHTING },
    stance: { eyeGapToLadder: EYE_GAP, mantleEyeGap: MANTLE_EYE_GAP, mantleRiseRungs: MANTLE_RISE,
      note: "tracked head held this far off the ladder face while climbing, then while planting on the deck" },
    handRotation: { rungPitchDeg: RUNG_PITCH, deckPitchDeg: DECK_PITCH, deckToeInDeg: DECK_TOE_IN,
      palm: "rolled down (overhand)", deck: "fingers forward, wrist raised" },
    rungGripOffset: { up: RUNG_GRIP_UP, towardClimber: RUNG_GRIP_OUT },
    stagingTeleport: STAGE, ladderSegments: ladders.map((l) => ({ model: l.model, rung: l.rungs[0] })),
    climb: { strokes }, mantle: { holds: ledges, pressFrames: pressPath.length, landed, gripsAfter: 0 },
    video: { file: "climb.mp4", fps: FPS, frames: captured }, inputs: "climb.inputs.json",
    gif: "climb.gif", samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ frames: captured, seconds: captured / FPS, landed, out }));
} finally {
  await game.shutdown();
}
