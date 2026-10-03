// Reproducible first-person VR movement clip for the website hero.
// From tools/shock2-sdk:
//   npm run build
//   DARK_ASSET_PATH=/path/to/25AE \
//     CARGO_TARGET_DIR=/path/to/target node scripts/hero-movement.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { faceTarget, GameServer } from "../dist/src/index.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "screenshots/hero");
const assetRoot = process.env.DARK_ASSET_PATH;
assert.ok(assetRoot, "set DARK_ASSET_PATH to the 25AE remaster asset root");
for (const file of ["sshock2.kpf", ...["400", "patch_ext", "scp", "shtup", "sshock2ee"].map((name) => `mods/${name}.kpf`)]) {
  assert.ok((await stat(resolve(assetRoot, file))).size > 0, `missing ${file}`);
}
const runtime = resolve(process.env.CARGO_TARGET_DIR ?? resolve(root, "target"), "debug/debug_runtime");
const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const revision = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
const framesDir = await mkdtemp(resolve(tmpdir(), "issue-1744-movement-"));
await mkdir(out, { recursive: true });

const game = await GameServer.launch({
  mission: "medsci1.mis",
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});
const samples = [];
const phases = [
  { name: "walk", count: 9, right: [0, 0.25], left: [0, 0], crouch: 0 },
  { name: "strafe-and-turn", count: 9, right: [-0.16, 0], left: [0.18, 0], crouch: 0 },
  { name: "crouch", count: 8, right: [0, 0], left: [0, 0], crouch: 1 },
  { name: "stand", count: 4, right: [0, 0], left: [-0.36, 0], crouch: 0 },
  { name: "jump", count: 15, right: [0, 0], left: [0, 0], crouch: 0 },
];
try {
  await game.step({ frames: 5 });
  await game.devParams.set("ambient_light_intensity", 0.9);
  await game.devParams.set("level_light_intensity", 2.0);
  await game.devParams.set("fov_override_deg", 85);
  await game.player.teleport({ x: 16, y: 1, z: 17 });
  await faceTarget(game, [11, 2, 17]);
  for (const [hand, x] of [["left", -0.2], ["right", 0.2]]) {
    await game.input.set(`${hand}_hand.position`, [x, 0.85, -0.5]);
  }
  await game.step({ frames: 10 });
  const start = (await game.info()).player;
  let n = 0;
  for (const phase of phases) {
    await game.input.set("right_hand.thumbstick", phase.right);
    await game.input.set("left_hand.thumbstick", phase.left);
    await game.input.set("crouch", phase.crouch);
    if (phase.name === "crouch") {
      for (const [hand, x] of [["left", -0.2], ["right", 0.2]]) {
        await game.input.set(`${hand}_hand.position`, [x, 0.35, -0.5]);
      }
    }
    if (phase.name === "stand") {
      for (const [hand, x] of [["left", -0.2], ["right", 0.2]]) {
        await game.input.set(`${hand}_hand.position`, [x, 0.85, -0.5]);
      }
    }
    if (phase.name === "jump") await game.input.trigger("LeftHandLowerButton");
    for (let i = 0; i < phase.count; i++) {
      await game.step({ frames: 4 });
      const image = resolve(framesDir, `${String(n).padStart(4, "0")}.png`);
      await game.screenshot(image, 1280);
      const { player } = await game.info();
      samples.push({ frame: n * 4 + 4, phase: phase.name,
        position: player.position, cameraOffset: player.camera_offset,
        rotation: player.rotation, image });
      n++;
    }
  }
  await game.input.set("right_hand.thumbstick", [0, 0]);
  await game.input.set("left_hand.thumbstick", [0, 0]);
  await game.input.set("crouch", 0);

  const first = samples[0];
  const walk = samples.findLast((s) => s.phase === "walk");
  const strafe = samples.findLast((s) => s.phase === "strafe-and-turn");
  const crouch = samples.findLast((s) => s.phase === "crouch");
  const stand = samples.findLast((s) => s.phase === "stand");
  const jump = samples.filter((s) => s.phase === "jump");
  const horizontal = (a, b) => Math.hypot(a.position[0] - b.position[0], a.position[2] - b.position[2]);
  assert.ok(horizontal(first, walk) > 0.5, "right stick must move the player forward");
  assert.ok(horizontal(walk, strafe) > 0.3, "strafe phase must move the player");
  assert.ok(crouch.cameraOffset[1] < strafe.cameraOffset[1] - 0.15, "crouch must lower the eye");
  assert.ok(stand.cameraOffset[1] > crouch.cameraOffset[1] + 0.15, "the player must stand before jumping");
  const maxJumpY = Math.max(...jump.map((s) => s.position[1]));
  assert.ok(maxJumpY > stand.position[1] + 0.12, "X button must launch the player");
  assert.ok(Math.abs(jump.at(-1).position[1] - stand.position[1]) < 0.1,
    "the player must land back on the original floor");
  assert.notDeepEqual(strafe.rotation, walk.rotation, "left stick must turn the player");
  assert.equal(n, 45);

  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p",
    "-movflags", "+faststart", resolve(out, "movement.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "movement.gif")]);
  await copyFile(first.image, resolve(out, "movement.png"));
  await writeFile(resolve(out, "movement.json"), JSON.stringify({
    mission: "medsci1.mis", presentation: "Vr", revision,
    runtimeSha256: await sha256(runtime), assetRoot,
    assetFiles: ["sshock2.kpf", "mods/400.kpf", "mods/patch_ext.kpf", "mods/scp.kpf", "mods/shtup.kpf", "mods/sshock2ee.kpf"],
    lighting: { ambient_light_intensity: 0.9, level_light_intensity: 2.0, fov_override_deg: 85 },
    stagingTeleport: [16, 1, 17], lookAt: [11, 2, 17], stagedPosition: start.position,
    input: { phases, jumpAction: "LeftHandLowerButton (Quest X; resolves to Jump)", framesPerSample: 4 },
    evidence: { forwardDistance: horizontal(first, walk), strafeDistance: horizontal(walk, strafe),
      standingEye: stand.cameraOffset[1], crouchedEye: crouch.cameraOffset[1],
      standingY: stand.position[1], peakJumpY: maxJumpY, endJumpY: jump.at(-1).position[1] },
    video: { file: "movement.mp4", fps: 15, frames: n, resolution: [960, 540] },
    still: "movement.png", gif: "movement.gif",
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ seconds: n / 15, frames: n, first: first.position,
    walk: walk.position, strafe: strafe.position, crouchEye: crouch.cameraOffset[1],
    standEye: stand.cameraOffset[1], peakJumpY: maxJumpY, out, framesDir }));
} finally {
  await game.shutdown();
}
