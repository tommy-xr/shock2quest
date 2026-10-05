// Reproducible first-person VR Toolbelt clip for the website, with the
// controller inputs recorded beside it (toolbelt.inputs.json): draw the belt
// tricorder, scan a locked security crate, play its hack board on the
// device's screen with the free hand, pull the loot out and stow it over the
// shoulder.
// From tools/shock2-sdk:
//   npm run build
//   DARK_ASSET_PATH=/path/to/25AE \
//     CARGO_TARGET_DIR=/path/to/target node scripts/hero-toolbelt.mjs
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
import { hasHackTexture as has } from "../dist/test/helpers/hack.js";
import { property } from "../dist/test/helpers/hrm.js";
import { carriedNaniteTotal } from "../dist/test/helpers/nanites.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const out = resolve(root, "screenshots/hero");
const clip = "toolbelt";
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
// The newest commit touching code the runtime is built from.
const runtimeRevision = git("log", "-1", "--format=%H", "--", "shock2vr", "engine", "dark", "runtimes");
const framesDir = await mkdtemp(resolve(tmpdir(), "hero-toolbelt-"));
await mkdir(out, { recursive: true });

// Recreation deck 1, by a railing: a locked security crate (mission object
// 190, holding a medical kit and a clip of standard bullets).
const mission = "rec1.mis";
const crateObject = 190;
const standOff = [0.4, 0, -2.0];
const fovDeg = 62;
const levelLight = 1.8;
// Provisioning: max Hack (and the Cyber stat behind it) so the authored board
// is winnable without mines, and nanites for its START.
const stats = { cyber_affinity: 6, skills: { hack: 6 } };
const stagedItems = ["Big Nanite Pile"];
// Idle hands (pawn-local).
const rest = { left: [-0.2, 0.8, -0.4], right: [0.26, 0.76, -0.4] };
// Relative to the eye (x right, y up, -z forward): the tricorder's lens while
// it scans, and the right controller while it points at the screen.
const lensFromEye = [-0.12, -0.1, -0.36];
const pointerFromEye = [0.13, -0.17, -0.3];
// Head pitch (degrees down) toward the crate.
const headPitch = 22;

// faceTarget steers the pawn continuously: smooth turning, no comfort mask,
// without touching the user's saved settings.
const settings = { vr: { turning: "Smooth", vignette: "Off" } };
const settingsDir = await mkdtemp(resolve(tmpdir(), "hero-toolbelt-settings-"));
process.env.SHOCK2_SETTINGS_PATH = resolve(settingsDir, "user-settings.json");
await writeFile(process.env.SHOCK2_SETTINGS_PATH, JSON.stringify(settings));

const game = await GameServer.launch({
  mission,
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});
const samples = [];
const gear = async () => (await game.info()).player.hand_feedback.body_gear.personal_card;
const nanites = () => carriedNaniteTotal(game);
try {
  await game.step({ frames: 5 });
  await game.devParams.set("cheat", 1);
  await game.devParams.set("fov_override_deg", fovDeg);
  await game.devParams.set("level_light_intensity", levelLight);
  await game.step({ frames: 30 });
  const [crate] = await game.entities.byTemplate(crateObject);
  assert.ok(crate, `rec1 has security crate ${crateObject}`);
  const loot = (await game.entities.detail(crate.id)).outgoing_links
    .filter((l) => l.link_type.startsWith("Contains")).map((l) => ({ id: l.target_id, name: l.target_name }));
  assert.ok(loot.length > 0, "the crate holds its authored loot");
  const lockedBefore = await property(game, crate.id, "ObjectState");
  assert.equal(lockedBefore, "Locked", "the crate starts locked");
  const scanAt = add(crate.position, [0, 0.15, 0]);

  // Measure the lens offset in the hand at an identity pose, at spawn where
  // the reference reach touches nothing.
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

  const spot = add(crate.position, [standOff[0], 1.2, standOff[2]]);
  await game.player.teleport({ x: spot[0], y: spot[1], z: spot[2] });
  await game.step({ frames: 30 });
  await faceTarget(game, scanAt);
  {
    const { player } = await game.info();
    const eyeY = player.position[1] + player.camera_offset[1];
    const reach = Math.hypot(scanAt[0] - player.position[0], scanAt[2] - player.position[2]);
    await game.input.lookAtWorldPoint([scanAt[0], eyeY - reach * Math.tan(headPitch * Math.PI / 180), scanAt[2]]);
  }
  for (const hand of ["left", "right"]) {
    await game.input.set(`${hand}_hand.position`, rest[hand]);
    await game.input.set(`${hand}_hand.rotation`, [0, 0, 0, 1]);
  }
  // Find the palm-corrected buckle pose from here, then put the device back.
  await drawPersonalCard(game, "left");
  const buckle = (await game.input.state()).left_hand.position;
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 10 });
  await game.player.setStats(stats);
  for (const name of stagedItems) await game.player.spawnItem(name);
  await game.step({ frames: 20 });
  const start = await game.info();
  const nanitesBefore = await nanites();
  assert.ok(nanitesBefore >= 10, `the wallet covers the board's START (has ${nanitesBefore})`);
  assert.equal((await gear()).hand, null);
  assert.equal(start.player.right_hand_entity_id, null);
  assert.ok(Math.hypot(...sub(start.player.position, crate.position).filter((_, i) => i !== 1)) < 3,
    "the crate stays within the panel's reach");

  // Lens pose aimed at the crate, solved into a left-hand pose.
  const eye = start.player.camera_offset[1];
  const lens = add(lensFromEye, [0, eye, 0]);
  const crateLocal = quatRotate(quatConjugate(start.player.rotation), sub(scanAt, start.player.position));
  const aimRotation = quatMultiply(quatFromTo([0, 0, -1], normalize(sub(crateLocal, lens))), quatConjugate(mount.rotation));
  const aimPosition = sub(lens, quatRotate(aimRotation, lensOffset));
  // Reading pose: the lens looks straight away from the eye, so the screen
  // faces it and the pointing hand meets the screen squarely.
  const readRotation = quatMultiply(quatFromTo([0, 0, -1], normalize(sub(lens, [0, eye, 0]))), quatConjugate(mount.rotation));
  const readPosition = sub(lens, quatRotate(readRotation, lensOffset));

  const capture = async (phase, count = 1) => {
    for (let i = 0; i < count; i++) {
      await game.step({ frames: 4 });
      const image = resolve(framesDir, `${String(samples.length).padStart(4, "0")}.png`);
      await game.screenshot(image, 1280);
      const ui = await game.ui.state();
      const { player } = await game.info();
      const card = player.hand_feedback.body_gear.personal_card;
      samples.push({ phase, image, mode: ui.mode, tricorderHand: card.hand, scans: card.scans,
        panel: ui.active_panel?.entity_id ?? null, hitPoints: player.hit_points });
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
  const panel = async () => (await game.ui.state()).active_panel;

  const inputs = recordClipInputs(game);
  inputs.start();
  // 1. Squeeze at the buckle, raise the tricorder and hold its lens on the
  //    crate for the 0.4 s focus scan: the crate's hack board opens on it.
  await game.input.set("left_hand.position", buckle);
  await capture("reach-buckle");
  await game.input.set("left_hand.squeeze", 1);
  await capture("draw");
  assert.equal((await gear()).hand, 0, "squeezing at the buckle draws the tricorder");
  await move("left", "raise", { position: buckle, rotation: [0, 0, 0, 1] },
    { position: aimPosition, rotation: aimRotation }, 4);
  let board;
  for (let i = 0; i < 12 && !board; i++) {
    await capture("scan");
    const p = await panel();
    if (p?.entity_id === crate.id) board = p;
  }
  assert.ok(board, "holding the lens on the crate opens its panel");
  assert.equal((await gear()).last_scan, crate.id, "the scan resolved the crate");
  assert.ok(has(board, "hack.pcx"), "the locked crate presents its hack board");
  assert.ok(!board.elements.some((e) => loot.some((l) => l.id === e.entity_id)), "its loot stays sealed");
  await move("left", "read", { position: aimPosition, rotation: aimRotation },
    { position: readPosition, rotation: readRotation }, 2);
  const screen = (await game.ui.state()).panel_pose;
  assert.ok(screen, "the board is on the device's screen");

  // 2. The empty right hand points at the screen: trigger START, then light
  //    nodes toward a connected three, choosing each from the live board.
  const handPosition = add(pointerFromEye, [0, eye, 0]);
  const pointing = (element) => {
    const [x, y, w, h] = element.rect;
    const at = add(screen.center, quatRotate(screen.rotation,
      [((x + w / 2) / screen.canvas[0] - 0.5) * screen.size[0], (0.5 - (y + h / 2) / screen.canvas[1]) * screen.size[1], 0]));
    return { position: handPosition, rotation: quatFromTo([0, 0, -1], sub(at, handPosition)) };
  };
  const button = (p, label) => {
    const found = p.elements.find((e) => e.kind === "button" && e.label === label);
    assert.ok(found, `the board shows ${label}`);
    return found;
  };
  let pose = { position: rest.right, rotation: [0, 0, 0, 1] };
  const click = async (phase, element, steps = 2) => {
    const to = pointing(element);
    await move("right", phase, pose, to, steps);
    pose = to;
    await game.input.set("right_hand.trigger", 1);
    await capture(phase);
    await game.input.set("right_hand.trigger", 0);
    await capture(phase);
  };
  // The board shows the crate's authored HackDiff cost.
  const cost = Number(board.elements.find((e) => e.kind === "text" && /^\d+$/.test(e.text ?? ""))?.text);
  assert.ok(cost > 0, "the board shows its cost");
  await click("start", button(board, "start-hack"), 3);
  assert.equal(await nanites(), nanitesBefore - cost, "START charges the authored cost");
  // Node states from the board art: hrmon = lit, hrmburn = burned, hrmmine =
  // a critical node that would ruin the crate.
  const nodeState = (p, label) => {
    const [x, y] = button(p, label).rect;
    const states = { "hrmon.pcx": "lit", "hrmburn.pcx": "burned", "hrmmine.pcx": "mine" };
    const art = p.elements.find((e) => e.kind === "image" && e.rect[0] === x && e.rect[1] === y
      && states[e.texture?.toLowerCase()]);
    return art ? states[art.texture.toLowerCase()] : "free";
  };
  const lines = [];
  for (let y = 0; y < 4; y++) for (let x = 0; x <= 2; x++) lines.push([0, 1, 2].map((d) => `node-${x + d}-${y}`));
  for (let x = 0; x < 5; x++) for (let y = 0; y <= 1; y++) lines.push([0, 1, 2].map((d) => `node-${x}-${y + d}`));
  const played = [];
  let opened;
  for (let attempt = 0; attempt < 12 && !opened; attempt++) {
    const p = await panel();
    assert.ok(!has(p, "loseh.pcx") && !has(p, "failh.pcx"), `the board stays winnable: ${JSON.stringify(played)}`);
    const labels = new Set(p.elements.filter((e) => e.label?.startsWith("node-")).map((e) => e.label));
    const open = lines.filter((line) => line.every((n) => labels.has(n) && ["lit", "free"].includes(nodeState(p, n))));
    assert.ok(open.length, "a connectable line remains");
    const score = (line) => line.filter((n) => nodeState(p, n) === "lit").length;
    // Play toward the line nearest completion.
    open.sort((a, b) => score(b) - score(a));
    const label = open[0].find((n) => nodeState(p, n) === "free");
    await click("hack", button(p, label));
    const after = await panel();
    assert.ok(after, `the panel stays open after ${label}`);
    if (after && has(after, "contain.pcx")) opened = after;
    played.push({ node: label, result: opened ? "won" : nodeState(after, label) });
  }
  assert.ok(opened, "completing the circuit opens the crate's loot on the device");
  for (const item of loot) {
    assert.ok(opened.elements.some((e) => e.entity_id === item.id), `the loot grid shows ${item.name}`);
  }
  const lockedAfter = await property(game, crate.id, "ObjectState");
  assert.equal(lockedAfter, "Hacked", "the hack unlocks the crate");
  await capture("loot", 3);

  // 3. Squeeze the clip out of the loot grid, then stow it over the right
  //    shoulder: released there, it goes into the backpack. (The medical kit would
  //    fill the frame this close to the lens.)
  const pulled = loot.find((l) => /Clip/i.test(l.name));
  assert.ok(pulled, "the crate holds a clip");
  const slot = opened.elements.find((e) => e.entity_id === pulled.id);
  const toSlot = pointing(slot);
  await move("right", "point-loot", pose, toSlot, 2);
  await game.input.set("right_hand.squeeze", 1);
  await capture("pull");
  assert.equal((await game.info()).player.right_hand_entity_id, pulled.id, "squeezing the slot pulls the clip out");
  const { player } = await game.info();
  const shoulder = player.hand_feedback.shoulder_backpack.centers[1];
  const toShoulder = {
    position: quatRotate(quatConjugate(player.rotation), sub(shoulder, player.position)),
    rotation: [0, 0, 0, 1],
  };
  // Swing out to the right first, so the clip leaves frame instead of passing
  // the lens on its way up.
  const aside = { position: add([0.45, -0.3, -0.15], [0, eye, 0]), rotation: [0, 0, 0, 1] };
  await move("right", "stow", toSlot, aside, 3);
  await move("right", "stow", aside, toShoulder, 2);
  assert.equal((await game.info()).player.hand_feedback.shoulder_backpack.near[1], true, "the hand reaches the shoulder");
  await game.input.set("right_hand.squeeze", 0);
  await capture("release");
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  const stowed = (await game.player.inventory()).items.some((i) => i.entity_id === pulled.id);
  assert.ok(stowed, "the released clip is in the backpack");
  assert.equal((await game.physics.bodies({ entityId: pulled.id })).bodies.length, 0, "no world body is left");
  await move("right", "lower", toShoulder, { position: rest.right, rotation: [0, 0, 0, 1] }, 2);
  await capture("loot", 2);

  // 3. Release: the tricorder returns to the belt; the hand ends at the
  //    buckle, where the clip starts, so it loops.
  const held = await gear();
  assert.equal(held.scans, 1, "the tricorder scanned only the crate");
  assert.equal(held.last_scan, crate.id);
  await game.input.set("left_hand.squeeze", 0);
  await capture("return");
  assert.equal((await gear()).hand, null, "releasing returns the tricorder to the belt");
  for (let i = 1; i <= 3; i++) {
    const t = ease(i / 3);
    await game.input.set("left_hand.position", lerp(readPosition, buckle, t));
    await game.input.set("left_hand.rotation", nlerp(readRotation, [0, 0, 0, 1], t));
    await capture("loop");
  }
  await inputs.write(resolve(out, `${clip}.inputs.json`));
  inputs.dispose();
  assert.ok(samples.every((s) => s.hitPoints === start.player.hit_points), "no damage during the clip");

  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-an",
    "-movflags", "+faststart", resolve(out, `${clip}.mp4`)]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, `${clip}.gif`)]);
  // Poster: aiming the final node, two already lit.
  const poster = samples.findLastIndex((s) => s.phase === "hack") - 2;
  await copyFile(samples[poster].image, resolve(out, `${clip}.png`));
  await writeFile(resolve(out, `${clip}.json`), JSON.stringify({
    mission, spot: { crateObject, crate: crate.position, standOff }, presentation: "Vr", revision, runtimeRevision,
    runtimeSha256: await sha256(runtime), assetRoot, assetFiles, settings,
    devParams: { cheat: 1, fov_override_deg: fovDeg, level_light_intensity: levelLight },
    provisioning: { stats, stagedItems, note: "debug setStats/spawnItem before the clip: max Hack + Cyber, nanites for START" },
    stagedPosition: start.player.position,
    input: { tricorder: "left squeeze at the buckle, lens held on the crate (focus scan), release",
      board: "right trigger on START and nodes via the hand ray on the device screen",
      loot: "right squeeze on the clip's slot, hand to the right shoulder zone, release",
      framesPerSample: 4, timeline: `${clip}.inputs.json` },
    evidence: { crate: { entity: crate.id, objState: { before: lockedBefore, after: lockedAfter } },
      nanites: { before: nanitesBefore, cost, afterStart: nanitesBefore - cost }, played, loot,
      stowed: { entity: pulled.id, name: pulled.name, inBackpack: stowed, worldBodies: 0 },
      hitPoints: start.player.hit_points },
    video: { file: `${clip}.mp4`, fps: 15, frames: samples.length, resolution: [960, 540] },
    still: `${clip}.png`, gif: `${clip}.gif`,
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ seconds: samples.length / 15, frames: samples.length, poster, played, out, framesDir }));
} finally {
  await game.shutdown();
  await rm(settingsDir, { recursive: true, force: true });
}
