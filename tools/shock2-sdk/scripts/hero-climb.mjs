// Capture an actual VR hand climb and mantle in medsci1 for the website hero.
// Run from tools/shock2-sdk after npm run build:
// DARK_ASSET_PATH=/path/to/25AE CARGO_TARGET_DIR=/path/to/target node scripts/hero-climb.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { GameServer, otherHand, vrClimbLadder, vrTopOut } from "../dist/src/index.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "screenshots/hero");
const framesDir = await mkdtemp(resolve(tmpdir(), "issue-1745-climb-"));
const samplesDir = resolve(tmpdir(), "issue-1745-climb-samples");
const assetRoot = process.env.DARK_ASSET_PATH;
assert.ok(assetRoot, "set DARK_ASSET_PATH to the 25AE remaster asset root");
for (const file of ["sshock2.kpf", ...["400", "patch_ext", "scp", "shtup", "sshock2ee"].map((name) => `mods/${name}.kpf`)]) {
  assert.ok((await stat(resolve(assetRoot, file))).size > 0, `missing ${file} from 25AE remaster assets`);
}
await mkdir(out, { recursive: true });
await mkdir(samplesDir, { recursive: true });

const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const revision = execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim();
const runtime = resolve(process.env.CARGO_TARGET_DIR ?? resolve(root, "target"), "debug/debug_runtime");
const game = await GameServer.launch({
  mission: "medsci1.mis",
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});

let tick = 0;
let captured = 0;
let vaultSeen = false;
let sinceVault = 0;
let settledFrames = 0;
const samples = [];
const rawStep = game.step.bind(game);
async function shot(phase) {
  const path = resolve(framesDir, `${String(captured).padStart(4, "0")}.png`);
  await game.screenshot(path, 1280);
  const { player } = await game.info();
  samples.push({ frame: tick, image: path, phase, position: player.position,
    grips: player.climb.grips.map(({ hand, kind }) => ({ hand, kind })),
    vaulting: player.climb.vaulting });
  captured++;
}
try {
  await rawStep({ frames: 5 });
  // Both are engine developer lighting controls; the geometry and materials
  // remain the production medsci1 scene. Record their values in provenance.
  await game.devParams.set("ambient_light_intensity", 0.9);
  await game.devParams.set("level_light_intensity", 2.0);
  await game.player.teleport({ x: -17.54, y: -4.5, z: 14.65 });
  await rawStep({ frames: 10 });
  await game.input.lookAtWorldPoint([-17.54, -1.5, 14.4]);
  const preroll = await vrClimbLadder(game, { near: [-17.54, -4, 14.5], untilY: -3.4 });
  assert.ok(preroll.heights.at(-1) >= -3.4);
  // A shoulder recall status line can fire while moving an empty hand into
  // place. Its ordinary HUD lifetime is five seconds; let it expire on the
  // held ladder before the clip starts.
  await rawStep({ frames: 330 });

  const stage = (await game.info()).player;
  assert.equal(stage.climb.grips.length, 1);
  assert.equal(stage.climb.grips[0].kind, "ladder");
  await game.input.lookAtWorldPoint([-17.54, stage.position[1] + 1.35, 14.45]);
  await rawStep({ frames: 1 });
  await shot("first-grip");

  game.step = async ({ frames = 1 } = {}) => {
    for (let i = 0; i < frames; i++) {
      const before = (await game.info()).player;
      // A headset wearer follows the next hold, then turns toward the deck
      // over twenty frames. Keep the deck gaze after the vault lands.
      const t = Math.min(1, sinceVault / 20);
      const climbTarget = [-17.54, before.position[1] + 1.35, 14.45];
      const deckTarget = [-17.54, before.position[1] - 0.2, 10.0];
      const target = climbTarget.map((c, axis) => c * (1 - t) + deckTarget[axis] * t);
      await game.input.lookAtWorldPoint(target);
      await rawStep({ frames: 1 });
      tick++;
      const after = (await game.info()).player;
      if (after.climb.vaulting) vaultSeen = true;
      if (vaultSeen) sinceVault++;
      if (vaultSeen && !after.climb.vaulting) settledFrames++;
      if (tick % 4 === 0 && settledFrames <= 20) {
        const phase = after.climb.vaulting ? "mantle"
          : vaultSeen ? "landed" : "hand-over-hand";
        await shot(phase);
      }
    }
  };

  const climb = await vrClimbLadder(game, { near: [-17.54, -4, 14.5], untilY: -2.0 });
  assert.ok(climb.heights.at(-1) >= -2.0);
  assert.equal((await game.info()).player.climb.grips[0].kind, "ladder");
  const ledgeHand = otherHand(climb.anchor);
  const ledgeHold = ledgeHand === "left"
    ? [-17.6, -1.5, 14.05] : [-17.35, -1.5, 14.05];
  const topOut = await vrTopOut(game, ledgeHand, ledgeHold);
  assert.ok(vaultSeen, "the ledge pull must trigger the mantle");
  assert.ok(topOut.landed[2] < 14.1, "the player must finish past the ladder lip");
  assert.equal((await game.info()).player.climb.grips.length, 0);
  assert.ok(captured >= 30 && captured <= 45, `expected a 2–3 second clip, got ${captured} frames`);

  const input = resolve(framesDir, "%04d.png");
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", input,
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p",
    "-movflags", "+faststart", resolve(out, "climb.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", input,
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "climb.gif")]);
  const named = [
    ["climb.png", samples.find((s) => s.phase === "first-grip")],
    ["climb-second-stroke.png", samples.find((s) => s.phase === "hand-over-hand" && s.frame >= 24)],
    ["climb-mantle.png", samples.find((s) => s.phase === "mantle")],
    ["climb-landed.png", samples.find((s) => s.phase === "landed")],
  ];
  for (const [name, sample] of named) {
    assert.ok(sample, `missing ${name} phase`);
    await copyFile(sample.image, name === "climb.png" ? resolve(out, name) : resolve(samplesDir, name));
  }
  await writeFile(resolve(out, "climb.json"), JSON.stringify({
    mission: "medsci1.mis", presentation: "Vr", revision, runtimeSha256: await sha256(runtime),
    assetRoot, lighting: { ambient_light_intensity: 0.9, level_light_intensity: 2.0 },
    stagingTeleport: [-17.54, -4.5, 14.65],
    preroll: { untilY: -3.4, anchor: preroll.anchor, endedAt: stage.position },
    recordedClimb: { near: [-17.54, -4, 14.5], untilY: -2.0,
      anchor: climb.anchor, firstY: climb.heights[0], lastY: climb.heights.at(-1) },
    mantle: { hand: ledgeHand, hold: ledgeHold, landed: topOut.landed, gripsAfter: 0 },
    video: { file: "climb.mp4", fps: 15, frames: captured },
    gif: "climb.gif", samplesDir, samples,
  }, null, 2) + "\n");
  console.log(JSON.stringify({ frames: captured, seconds: captured / 15, landed: topOut.landed,
    out, phaseImages: named.map(([name]) => name) }));
} finally {
  game.step = rawStep;
  await game.shutdown();
}
