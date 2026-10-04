// Reproducible first-person VR MFD clip for the website hero, with the
// controller inputs recorded beside it (mfd.inputs.json): draw the belt
// tricorder and scan a mug, then jack into the cyber interface and move an
// inventory item.
// From tools/shock2-sdk:
//   npm run build
//   DARK_ASSET_PATH=/path/to/25AE \
//     CARGO_TARGET_DIR=/path/to/target node scripts/hero-mfd.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { faceTarget, GameServer, recordClipInputs } from "../dist/src/index.js";
import {
  add, drawPersonalCard, normalize, quatConjugate, quatFromTo, quatMultiply, quatNormalize, quatRotate, scale, sub,
} from "../dist/test/helpers/vr-hand.js";

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
const framesDir = await mkdtemp(resolve(tmpdir(), "hero-mfd-"));
await mkdir(out, { recursive: true });

// Engineering deck 1's opening office: a desk with a Comp-Pad and a mug.
const mission = "eng1.mis";
const spot = [44.9, -15.36, -41.0];
const fovDeg = 62;
const levelLight = 1.3;
// Idle hands (pawn-local). The idle left hand tips 35 degrees down so its
// ray stays off the interface.
const rest = { left: [-0.2, 0.8, -0.4], right: [0.26, 0.76, -0.4] };
const leftRestRotation = [-Math.sin(35 / 360 * Math.PI), 0, 0, Math.cos(35 / 360 * Math.PI)];
// Relative to the eye (x right, y up, -z forward): the tricorder's lens while
// it scans, and the right controller while it points at the interface.
const lensFromEye = [-0.07, -0.08, -0.38];
const pointerFromEye = [0.2, -0.32, -0.45];
// Head pitch (degrees down): toward the mug while scanning, then near level
// for the interface, which opens at eye height.
const headPitch = { scan: 16, jackIn: 3 };
const stagedItems = ["Wrench", "Pistol", "Med Patch", "Small Standard Clip"];

// faceTarget steers the pawn continuously: smooth turning, no comfort mask,
// without touching the user's saved settings.
const settings = { vr: { turning: "Smooth", vignette: "Off" } };
const settingsDir = await mkdtemp(resolve(tmpdir(), "hero-mfd-settings-"));
process.env.SHOCK2_SETTINGS_PATH = resolve(settingsDir, "user-settings.json");
await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify(settings));

const game = await GameServer.launch({
  mission,
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});
const samples = [];
const gear = async () => (await game.info()).player.hand_feedback.body_gear.personal_card;
try {
  await game.step({ frames: 5 });
  await game.devParams.set("cheat", 1);
  await game.devParams.set("fov_override_deg", fovDeg);
  await game.devParams.set("level_light_intensity", levelLight);
  // Let props settle onto the desk before reading positions.
  await game.step({ frames: 60 });
  const { entities } = await game.entities.list({ limit: 5000 });
  // eng1 has several mugs: take the desk's.
  const mug = entities.find((e) => e.name === "Mug" && Math.hypot(...sub(e.position, spot)) < 3);
  assert.ok(mug, "eng1 has its desk mug");
  const scanAt = add(mug.position, [0, 0.08, 0]);

  // Measure the lens offset in the hand at an identity pose, here at spawn
  // where the reference reach touches nothing (by the desk it knocks the mug off).
  for (const hand of ["left", "right"]) await game.input.set(`${hand}_hand.position`, rest[hand]);
  await drawPersonalCard(game, "left");
  const reference = [0, 0.4, -0.7];
  await game.input.set("left_hand.position", reference);
  await game.step({ frames: 3 });
  const mount = (await game.ui.state()).scanner_pose;
  assert.ok(mount, "the drawn tricorder reports its lens");
  const lensOffset = sub(mount.origin, reference);
  await game.input.set("left_hand.squeeze", 0);
  await game.input.set("left_hand.position", rest.left);
  await game.step({ frames: 10 });

  await game.player.teleport({ x: spot[0], y: spot[1], z: spot[2] });
  await game.step({ frames: 20 });
  await faceTarget(game, scanAt);
  const lookPitch = async (degrees) => {
    const { player } = await game.info();
    const eyeY = player.position[1] + player.camera_offset[1];
    const reach = Math.hypot(scanAt[0] - player.position[0], scanAt[2] - player.position[2]);
    await game.input.lookAtWorldPoint([scanAt[0], eyeY - reach * Math.tan(degrees * Math.PI / 180), scanAt[2]]);
  };
  await lookPitch(headPitch.scan);
  for (const hand of ["left", "right"]) {
    await game.input.set(`${hand}_hand.position`, rest[hand]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  }
  // Find the palm-corrected buckle pose from here, then put the device back.
  await drawPersonalCard(game, "left");
  const buckle = (await game.input.state()).left_hand.position;
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 10 });
  const wrench = await game.player.spawnItem(stagedItems[0]);
  for (const name of stagedItems.slice(1)) await game.player.spawnItem(name);
  await game.step({ frames: 20 });
  const start = await game.info();
  assert.equal((await gear()).hand, null);
  assert.equal(start.player.right_hand_entity_id, null);

  // Lens pose aimed at the mug, solved into a left-hand pose.
  const eye = start.player.camera_offset[1];
  const lens = add(lensFromEye, [0, eye, 0]);
  const mugLocal = quatRotate(quatConjugate(start.player.rotation), sub(scanAt, start.player.position));
  const aimRotation = quatMultiply(quatFromTo([0, 0, -1], normalize(sub(mugLocal, lens))), quatConjugate(mount.rotation));
  const aimPosition = sub(lens, quatRotate(aimRotation, lensOffset));

  const capture = async (phase, count = 1) => {
    for (let i = 0; i < count; i++) {
      await game.step({ frames: 4 });
      const image = resolve(framesDir, `${String(samples.length).padStart(4, "0")}.png`);
      await game.screenshot(image, 1280);
      const ui = await game.ui.state();
      const { player } = await game.info();
      const card = player.hand_feedback.body_gear.personal_card;
      samples.push({ phase, image, mode: ui.mode, tricorderHand: card.hand, scans: card.scans,
        held: player.right_hand_entity_id, hitPoints: player.hit_points,
        preview: ui.strip?.elements.find((e) => e.label === "RELEASE TO PLACE")?.rect ?? null });
    }
  };
  const ease = (t) => t * t * (3 - 2 * t);
  const lerp = (a, b, t) => add(scale(a, 1 - t), scale(b, t));
  const nlerp = (a, b, t) => quatNormalize(a.map((v, i) => v * (1 - t) + b[i] * t));
  const move = async (hand, phase, from, to, steps) => {
    for (let i = 1; i <= steps; i++) {
      const t = ease(i / steps);
      await game.input.set(`${hand}_hand.position`, lerp(from.position, to.position, t));
      await game.input.set(`${hand}_hand.rotation`, nlerp(from.rotation, to.rotation, t));
      await capture(phase);
    }
  };

  const inputs = recordClipInputs(game);
  inputs.start();
  // 1. Belt tricorder: squeeze at the buckle, raise it, hold the lens on the
  //    mug for the 0.4 s focus scan, read it, let go.
  await game.input.set("left_hand.position", buckle);
  await capture("reach-buckle");
  await game.input.set("left_hand.squeeze", 1);
  await capture("draw");
  assert.equal((await gear()).hand, 0, "squeezing at the buckle draws the tricorder");
  await move("left", "raise", { position: buckle, rotation: [0, 0, 0, 1] },
    { position: aimPosition, rotation: aimRotation }, 4);
  await capture("scan", 8);
  const scanned = await gear();
  assert.equal(scanned.scans, 1, "holding the lens on the mug scans it");
  assert.equal(scanned.last_scan, mug.id);
  await capture("readout", 4);
  await game.input.set("left_hand.squeeze", 0);
  await capture("return");
  assert.equal((await gear()).hand, null, "releasing returns the tricorder to the belt");
  assert.equal((await game.ui.state()).panel_pose, null, "the returned tricorder closes its screen");
  for (let i = 1; i <= 3; i++) {
    const t = ease(i / 3);
    await game.input.set("left_hand.position", lerp(aimPosition, rest.left, t));
    await game.input.set("left_hand.rotation", nlerp(aimRotation, leftRestRotation, t));
    await lookPitch(headPitch.scan + (headPitch.jackIn - headPitch.scan) * t);
    await capture("look-up");
  }

  // 2. Cyber interface: tap Menu, grip the wrench, drag its ghost footprint
  //    to another cell, release, tap Menu again.
  await game.input.hold("MenuButton");
  await capture("menu-in", 4);
  await game.input.release("MenuButton");
  await capture("open", 3);
  let ui = await game.ui.state();
  assert.equal(ui.mode, "use", "a Menu tap opens the cyber interface");
  const panel = ui.panel_pose;
  assert.ok(panel);
  const slot = ui.strip.elements.find((e) => e.entity_id === wrench.entity_id);
  assert.ok(slot, "the staged wrench is in the inventory");
  const source = [slot.rect[0] + slot.rect[2] / 2, slot.rect[1] + slot.rect[3] / 2];
  const target = [source[0] + 6 * slot.rect[2], source[1]];
  // The controller stays low and aims up at the production panel pose
  // (both pawn-local, like hand input).
  const handPosition = add(pointerFromEye, [0, eye, 0]);
  const pointing = (canvas) => {
    const at = add(panel.center, quatRotate(panel.rotation,
      [(canvas[0] / panel.canvas[0] - 0.5) * panel.size[0], (0.5 - canvas[1] / panel.canvas[1]) * panel.size[1], 0]));
    return { position: handPosition, rotation: quatFromTo([0, 0, -1], sub(at, handPosition)) };
  };
  await move("right", "point", { position: rest.right, rotation: [0, 0, 0, 1] }, pointing(source), 3);
  await capture("point");
  await game.input.set("right_hand.squeeze", 1);
  await capture("grip", 2);
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.entity_id, "grip takes the wrench out");
  for (let i = 1; i <= 6; i++) {
    const { position, rotation } = pointing([source[0] + (target[0] - source[0]) * ease(i / 6), source[1]]);
    await game.input.set("right_hand.position", position);
    await game.input.set("right_hand.rotation", rotation);
    await capture("drag");
  }
  await capture("ghost-footprint", 5);
  const preview = (await game.ui.state()).strip.elements.find((e) => e.label === "RELEASE TO PLACE");
  assert.ok(preview, "the held wrench shows a placement ghost");
  await game.input.set("right_hand.squeeze", 0);
  await capture("place", 5);
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  const after = (await game.ui.state()).strip.elements.find((e) => e.entity_id === wrench.entity_id);
  assert.ok(after);
  assert.notEqual(after.rect[0], slot.rect[0], "the same wrench moves cells");
  for (let i = 0; i < 4; i++) assert.ok(Math.abs(after.rect[i] - preview.rect[i]) < 0.01, "it lands in the ghost's rect");
  assert.equal((await game.physics.bodies({ entityId: wrench.entity_id })).bodies.length, 0, "no world body is left");
  await move("right", "lower", pointing(target), { position: rest.right, rotation: [0, 0, 0, 1] }, 2);
  await game.input.hold("MenuButton");
  await capture("menu-out", 4);
  await game.input.release("MenuButton");
  await capture("closed");
  // End where the clip starts - left hand at the buckle, eyes on the mug -
  // so it loops.
  for (let i = 1; i <= 3; i++) {
    const t = ease(i / 3);
    await game.input.set("left_hand.position", lerp(rest.left, buckle, t));
    await game.input.set("left_hand.rotation", nlerp(leftRestRotation, [0, 0, 0, 1], t));
    await lookPitch(headPitch.jackIn + (headPitch.scan - headPitch.jackIn) * t);
    await capture("loop");
  }
  assert.notEqual((await game.ui.state()).mode, "use", "a second Menu tap closes the interface");
  await inputs.write(resolve(out, "mfd.inputs.json"));
  inputs.dispose();
  assert.ok(samples.every((s) => s.hitPoints === start.player.hit_points), "no damage during the clip");

  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an",
    "-movflags", "+faststart", resolve(out, "mfd.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "mfd.gif")]);
  const poster = samples.findLastIndex((s) => s.phase === "readout");
  await copyFile(samples[poster].image, resolve(out, "mfd.png"));
  await writeFile(resolve(out, "mfd.json"), JSON.stringify({
    mission, presentation: "Vr", revision, revisionParents,
    runtimeSha256: await sha256(runtime), assetRoot, assetFiles, settings,
    devParams: { cheat: 1, fov_override_deg: fovDeg, level_light_intensity: levelLight },
    stagingTeleport: spot, stagedPosition: start.player.position, stagedItems,
    input: { tricorder: "left squeeze at the buckle, lens held on the mug, release",
      menu: "MenuButton held 4 video frames (16 sim frames, under the 0.5 s pause hold), then released; in, and again out",
      inventory: { source, target }, framesPerSample: 4, timeline: "mfd.inputs.json" },
    evidence: { scanned: { entity: mug.id, name: mug.name, scans: scanned.scans },
      wrench: { entity: wrench.entity_id, before: slot.rect, preview: preview.rect, after: after.rect },
      noWorldBodyAfterDeposit: true, hitPoints: start.player.hit_points },
    video: { file: "mfd.mp4", fps: 15, frames: samples.length, resolution: [960, 540] },
    still: "mfd.png", gif: "mfd.gif",
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ seconds: samples.length / 15, frames: samples.length, poster, out, framesDir }));
} finally {
  await game.shutdown();
  await rm(settingsDir, { recursive: true, force: true });
}
