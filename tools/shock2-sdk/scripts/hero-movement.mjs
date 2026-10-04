// Reproducible first-person VR movement clip for the website hero, with the
// controller inputs recorded beside it (movement.inputs.json).
// From tools/shock2-sdk:
//   npm run build
//   DARK_ASSET_PATH=/path/to/25AE \
//     CARGO_TARGET_DIR=/path/to/target node scripts/hero-movement.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { faceTarget, GameServer, recordClipInputs } from "../dist/src/index.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "screenshots/hero");
const assetRoot = process.env.DARK_ASSET_PATH;
assert.ok(assetRoot, "set DARK_ASSET_PATH to the 25AE remaster asset root");
const assetFiles = ["sshock2.kpf", ...["400", "patch_ext", "scp", "shtup", "sshock2ee"].map((name) => `mods/${name}.kpf`)];
for (const file of assetFiles) {
  assert.ok((await stat(resolve(assetRoot, file))).size > 0, `missing ${file}`);
}
const runtime = resolve(process.env.CARGO_TARGET_DIR ?? resolve(root, "target"), "debug/debug_runtime");
const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
const revision = git("rev-parse", "HEAD");
const revisionParents = git("rev-parse", "HEAD^@").split("\n");
const framesDir = await mkdtemp(resolve(tmpdir(), "hero-movement-"));
await mkdir(out, { recursive: true });

// Recreation deck, the mall's lit shop street: wall-washed shopfronts, plants
// and the "neural implants express" sign. The deck's patrolling robots and
// hybrids would walk into frame, so those near the spot are cleared first.
const mission = "rec3.mis";
const spot = [104.5, -4, -127.5];
const lookAt = [90, -3, -134];
const cleared = ["Security", "Assault", "OG-Grenade", "Protocol Droid"];
const clearRadius = 45;
const fovDeg = 85;
// A touch over the authored light level: the mall is moody and reads dim on video.
const levelLight = 1.4;
// Stick values are game input, where x is the negated physical axis (the Quest
// runtime flips it): x > 0 is a push to the left, so strafe and turn go left.
// Crouch toggles like the Quest's left stick click.
const phases = [
  { name: "walk", count: 7, right: [0, 0.7], left: [0, 0], crouch: 0 },
  { name: "strafe", count: 6, right: [0.6, 0], left: [0, 0], crouch: 0 },
  { name: "turn", count: 5, right: [0, 0], left: [0.8, 0], crouch: 0 },
  { name: "crouch", count: 6, right: [0, 0], left: [0, 0], crouch: 1 },
  { name: "stand", count: 5, right: [0, 0], left: [-0.8, 0], crouch: 0 },
  // Long enough to land (~0.8 s here) and settle.
  { name: "jump", count: 14, right: [0, 0], left: [0, 0], crouch: 0 },
];
const jumpAction = "LeftHandLowerButton";
const handHeight = { stand: 0.85, crouch: 0.35 };

// faceTarget steers continuously: capture with smooth turning and no comfort
// mask, without touching the user's saved settings.
const settings = { vr: { turning: "Smooth", vignette: "Off" } };
const settingsDir = await mkdtemp(resolve(tmpdir(), "hero-movement-settings-"));
process.env.SHOCK2_SETTINGS_PATH = resolve(settingsDir, "user-settings.json");
await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify(settings));

const game = await GameServer.launch({
  mission,
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});
const setHands = async (y) => {
  for (const [hand, x] of [["left", -0.2], ["right", 0.2]]) {
    await game.input.set(`${hand}_hand.position`, [x, y, -0.5]);
  }
};
const samples = [];
try {
  // Turning arms after a neutral stick frame on a fresh settings load.
  await game.step({ frames: 1 });
  await game.devParams.set("cheat", 1);
  await game.devParams.set("fov_override_deg", fovDeg);
  await game.devParams.set("level_light_intensity", levelLight);
  const { entities } = await game.entities.list({ limit: 5000 });
  const near = (e) => Math.hypot(e.position[0] - spot[0], e.position[2] - spot[2]) < clearRadius;
  const victims = entities.filter((e) => cleared.includes(e.name) && near(e));
  for (const e of victims) await game.entities.sendMessage(e.id, { type: "Damage", amount: 1000 });
  // Let their deaths play out before staging.
  await game.step({ frames: 120 });
  await game.player.teleport({ x: spot[0], y: spot[1], z: spot[2] });
  await game.step({ frames: 20 });
  await faceTarget(game, lookAt);
  await setHands(handHeight.stand);
  await game.step({ frames: 10 });
  const start = (await game.info()).player;

  const inputs = recordClipInputs(game);
  inputs.start();
  let n = 0;
  for (const phase of phases) {
    await game.input.set("right_hand.thumbstick", phase.right);
    await game.input.set("left_hand.thumbstick", phase.left);
    await game.input.set("crouch", phase.crouch);
    if (phase.name === "crouch") await setHands(handHeight.crouch);
    if (phase.name === "stand") await setHands(handHeight.stand);
    if (phase.name === "jump") await game.input.trigger(jumpAction);
    for (let i = 0; i < phase.count; i++) {
      await game.step({ frames: 4 });
      const image = resolve(framesDir, `${String(n).padStart(4, "0")}.png`);
      await game.screenshot(image, 1280);
      const { player } = await game.info();
      samples.push({ frame: n * 4 + 4, phase: phase.name, position: player.position,
        cameraOffset: player.camera_offset, rotation: player.rotation, image });
      n++;
    }
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.input.set("left_hand.thumbstick", [0, 0]);
  await game.input.set("crouch", 0);
  await inputs.write(resolve(out, "movement.inputs.json"));
  inputs.dispose();

  const last = (name) => samples.findLast((s) => s.phase === name);
  const first = samples[0];
  const [walk, strafe, turn, crouch, stand] = ["walk", "strafe", "turn", "crouch", "stand"].map(last);
  const jump = samples.filter((s) => s.phase === "jump");
  const horizontal = (a, b) => Math.hypot(a.position[0] - b.position[0], a.position[2] - b.position[2]);
  // Signed: forward along the staged heading, then to its left (+z when facing -x).
  const heading = [lookAt[0] - start.position[0], lookAt[2] - start.position[2]];
  const along = (a, b, [x, z]) => ((b.position[0] - a.position[0]) * x + (b.position[2] - a.position[2]) * z) / Math.hypot(x, z);
  assert.ok(along(start, walk, heading) > 0.5, "right stick must move the player forward");
  assert.ok(along(walk, strafe, [heading[1], -heading[0]]) > 0.3, "right stick x must strafe the player left");
  // Yaw grows turning left; the stand phase turns back.
  const yaw = (s) => 2 * Math.atan2(s.rotation[1], s.rotation[3]);
  assert.ok(yaw(turn) > yaw(strafe) + 0.2, "left stick must turn the player left");
  assert.ok(Math.abs(yaw(stand) - yaw(strafe)) < 0.05, "left stick must turn the player back");
  assert.ok(crouch.cameraOffset[1] < turn.cameraOffset[1] - 0.15, "crouch must lower the eye");
  assert.ok(stand.cameraOffset[1] > crouch.cameraOffset[1] + 0.15, "the player must stand before jumping");
  const maxJumpY = Math.max(...jump.map((s) => s.position[1]));
  assert.ok(maxJumpY > stand.position[1] + 0.12, "X button must launch the player");
  assert.ok(Math.abs(jump.at(-1).position[1] - stand.position[1]) < 0.1,
    "the player must land back on the original floor");
  assert.equal(n, phases.reduce((sum, p) => sum + p.count, 0));

  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an",
    "-movflags", "+faststart", resolve(out, "movement.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "movement.gif")]);
  await copyFile(first.image, resolve(out, "movement.png"));
  await writeFile(resolve(out, "movement.json"), JSON.stringify({
    mission, presentation: "Vr", revision, revisionParents,
    runtimeSha256: await sha256(runtime), assetRoot, assetFiles, settings,
    devParams: { cheat: 1, fov_override_deg: fovDeg, level_light_intensity: levelLight },
    cleared: victims.map((e) => ({ name: e.name, position: e.position })),
    stagingTeleport: spot, lookAt, stagedPosition: start.position,
    input: { phases, jumpAction: `${jumpAction} (Quest X; resolves to Jump)`, framesPerSample: 4,
      timeline: "movement.inputs.json" },
    evidence: { forwardDistance: horizontal(first, walk), strafeDistance: horizontal(walk, strafe),
      standingEye: stand.cameraOffset[1], crouchedEye: crouch.cameraOffset[1],
      standingY: stand.position[1], peakJumpY: maxJumpY, endJumpY: jump.at(-1).position[1] },
    video: { file: "movement.mp4", fps: 15, frames: n, resolution: [960, 540] },
    still: "movement.png", gif: "movement.gif",
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ seconds: n / 15, frames: n, cleared: victims.length, first: first.position,
    walk: walk.position, strafe: strafe.position, crouchEye: crouch.cameraOffset[1],
    standEye: stand.cameraOffset[1], peakJumpY: maxJumpY, out, framesDir }));
} finally {
  await game.shutdown();
  await rm(settingsDir, { recursive: true, force: true });
}
