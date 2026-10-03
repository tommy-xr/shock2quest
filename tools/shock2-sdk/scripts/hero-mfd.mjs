// Reproducible first-person VR mfd clip for the website hero.
// From tools/shock2-sdk:
//   npm run build
//   DARK_ASSET_PATH=/path/to/25AE \
//     CARGO_TARGET_DIR=/path/to/target node scripts/hero-mfd.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFile, mkdir, mkdtemp, readFile, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { GameServer } from "../dist/src/index.js";

import { aimVrHandAtCanvas, add, sub, quatRotate, quatFromTo } from "../dist/test/helpers/vr-hand.js";

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
const framesDir = await mkdtemp(resolve(tmpdir(), "issue-1746-mfd-"));
await mkdir(out, { recursive: true });

const game = await GameServer.launch({
  mission: "medsci1.mis",
  debugFlags: ["--vr", "--window-size", "1280x720"],
  repoRoot: root,
});
const samples = [];
try {
  await game.step({ frames: 5 });
  await game.devParams.set("ambient_light_intensity", 0.9);
  await game.devParams.set("level_light_intensity", 2.0);
  await game.devParams.set("fov_override_deg", 60);
  await game.player.teleport({ x: 16, y: 1, z: 17 });
  await game.input.set("head.look", [90, 0]);
  for (const [hand, x] of [["left", -0.2], ["right", 0.2]]) {
    await game.input.set(`${hand}_hand.position`, [x, 0.85, -0.5]);
  }
  const wrench = await game.player.spawnItem("Wrench");
  for (const name of ["Pistol", "Med Patch", "Small Standard Clip"]) await game.player.spawnItem(name);
  await game.step({ frames: 10 });
  const capture = async (phase, count) => {
    for (let i = 0; i < count; i++) {
      await game.step({ frames: 4 });
      const image = resolve(framesDir, `${String(samples.length).padStart(4, "0")}.png`);
      await game.screenshot(image, 1280);
      const ui = await game.ui.state();
      samples.push({ phase, image, mode: ui.mode, pointer: ui.pointer,
        held: (await game.info()).player.right_hand_entity_id,
        preview: ui.strip?.elements.find(e => e.label === "RELEASE TO PLACE") ?? null });
    }
  };
  assert.notEqual((await game.ui.state()).mode, "use");
  await capture("closed", 3);
  await game.input.hold("MenuButton");
  await capture("menu-short-press", 2);
  await game.input.release("MenuButton");
  await capture("open", 5);
  let ui = await game.ui.state();
  assert.equal(ui.mode, "use", "Menu release must open the interface");
  const panel = ui.panel_pose;
  assert.ok(panel);
  const slot = ui.strip.elements.find(e => e.entity_id === wrench.entity_id);
  assert.ok(slot);
  const source = [slot.rect[0] + slot.rect[2] / 2, slot.rect[1] + slot.rect[3] / 2];
  const target = [source[0] + 6 * slot.rect[2], source[1]];
  await aimVrHandAtCanvas(game, panel, [320, 240], { hand: "left", facing: "away" });
  // Keep the controller below the inventory and aim upward, like a seated
  // player. Translate no UI geometry: targets use the production panel pose.
  const handPosition = add(panel.center, quatRotate(panel.rotation,
    [panel.size[0] * .25, -panel.size[1] * .22, .6]));
  const pointAt = async (canvas, squeeze = 0) => {
    const target = add(panel.center, quatRotate(panel.rotation,
      [(canvas[0] / panel.canvas[0] - .5) * panel.size[0],
       (.5 - canvas[1] / panel.canvas[1]) * panel.size[1], 0]));
    await game.input.set("right_hand.position", handPosition);
    await game.input.set("right_hand.rotation", quatFromTo([0, 0, -1], sub(target, handPosition)));
    await game.input.set("right_hand.squeeze", squeeze);
  };
  await pointAt(source);
  await capture("point", 5);
  await game.input.set("right_hand.squeeze", 1);
  await capture("grip", 4);
  assert.equal((await game.info()).player.right_hand_entity_id, wrench.entity_id);
  for (let i = 1; i <= 10; i++) {
    const t = i / 10;
    await pointAt([source[0] + (target[0] - source[0]) * t, source[1]], 1);
    await capture("drag", 1);
  }
  await capture("ghost-footprint", 7);
  const preview = (await game.ui.state()).strip.elements.find(e => e.label === "RELEASE TO PLACE");
  assert.ok(preview, "the held item must have a valid placement ghost");
  await game.input.set("right_hand.squeeze", 0);
  await capture("release", 9);
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  const after = (await game.ui.state()).strip.elements.find(e => e.entity_id === wrench.entity_id);
  assert.ok(after);
  assert.notEqual(after.rect[0], slot.rect[0], "the same item must move slots");
  for (let i = 0; i < 4; i++) assert.ok(Math.abs(after.rect[i] - preview.rect[i]) < .01);
  assert.equal((await game.physics.bodies({ entityId: wrench.entity_id })).bodies.length, 0);
  assert.equal(samples.length, 45);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=960:-2:flags=lanczos", "-c:v", "libx264", "-pix_fmt", "yuv420p",
    "-movflags", "+faststart", resolve(out, "mfd.mp4")]);
  execFileSync("ffmpeg", ["-y", "-loglevel", "error", "-framerate", "15", "-i", resolve(framesDir, "%04d.png"),
    "-vf", "scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
    resolve(out, "mfd.gif")]);
  await copyFile(samples[30].image, resolve(out, "mfd.png"));
  await writeFile(resolve(out, "mfd.json"), JSON.stringify({
    mission: "medsci1.mis", presentation: "Vr", revision, assetRoot,
    runtimeSha256: await sha256(runtime),
    assetFiles: ["sshock2.kpf", "mods/400.kpf", "mods/patch_ext.kpf", "mods/scp.kpf", "mods/shtup.kpf", "mods/sshock2ee.kpf"],
    lighting: { ambient_light_intensity: .9, level_light_intensity: 2, fov_override_deg: 60 },
    stagingTeleport: [16, 1, 17], headLook: [90, 0],
    stagedItems: ["Wrench", "Pistol", "Med Patch", "Small Standard Clip"],
    input: { menu: "MenuButton held 8 frames then released", source, target, framesPerSample: 4 },
    evidence: { entity: wrench.entity_id, before: slot.rect, preview: preview.rect, after: after.rect, noWorldBodyAfterDeposit: true },
    video: { file: "mfd.mp4", fps: 15, frames: 45, resolution: [960, 540] },
    still: "mfd.png", gif: "mfd.gif",
    samples: samples.map(({ image, ...sample }) => sample),
  }, null, 2) + "\n");
  console.log(JSON.stringify({ framesDir, out, before: slot.rect, after: after.rect }));
} finally {
  await game.shutdown();
}
