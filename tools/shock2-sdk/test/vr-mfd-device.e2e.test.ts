import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { test } from "node:test";
import { AimOcclusionError, GameServer } from "../src/index.js";
import type { SceneListResult } from "../src/types.js";
import { add, aimVrHandAt, drawPersonalCard, quatMultiply, quatRotate, scale } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
const shots = process.env.MFD_DEVICE_SHOTS;

/** Hold the drawn device up in front of the face: the left palm (+X) turned
 * up and back toward the eye, fingers (and the screen's top) pointing up and
 * away. Rz(90) turns the palm up; Rx(53) tilts it toward the eye. */
async function raiseDevice(game: GameServer) {
  const eye = (await game.info()).player.camera_offset[1];
  const s = Math.sin((53 / 2) * Math.PI / 180), c = Math.cos((53 / 2) * Math.PI / 180);
  const palmToEye = quatMultiply([s, 0, 0, c], [0, 0, Math.SQRT1_2, Math.SQRT1_2]);
  const p = Math.sin((-40 / 2) * Math.PI / 180), q = Math.cos((-40 / 2) * Math.PI / 180);
  await game.input.set("head.rotation", [p, 0, 0, q]);
  await game.input.set("left_hand.position", [0, eye - 0.22, -0.3]);
  await game.input.set("left_hand.rotation", palmToEye);
  await game.step({ frames: 4 });
}

async function face(game: GameServer) {
  const card = (await game.info()).player.hand_feedback?.body_gear?.personal_card;
  assert.ok(card?.device_face, "held device must report its face");
  return card.device_face;
}

/** Retail bottom-bar utility rects on the 640x480 canvas (mfd_utilities.rs). */
const MFD_BUTTON: [number, number] = [460 + 16, 430 + 20];
const RES_BUTTON: [number, number] = [117 + 16, 431 + 20];

/** Aim the right hand's ray at a use-mode canvas point shown on the device:
 * canvas -> face pixels through the reported window, then onto the face. */
async function aimAtDeviceCanvas(game: GameServer, canvas: [number, number], trigger = 0) {
  const f = await face(game);
  const window = f.windows.find(({ src: [x, y, w, h] }) =>
    canvas[0] >= x && canvas[0] <= x + w && canvas[1] >= y && canvas[1] <= y + h);
  assert.ok(window, `canvas point ${canvas} is not shown on the device`);
  const [sx, sy, sw, sh] = window.src, [dx, dy, dw, dh] = window.dst;
  const px = dx + ((canvas[0] - sx) / sw) * dw, py = dy + ((canvas[1] - sy) / sh) * dh;
  const u = px / f.canvas[0] - 0.5, v = 0.5 - py / f.canvas[1];
  const target = add(f.center, quatRotate(f.rotation, [u * f.size[0], v * f.size[1], 0]));
  await aimVrHandAt(game, target, 0.25, 0, trigger, { lookAtTarget: false });
}

async function clickDevice(game: GameServer, canvas: [number, number]) {
  await aimAtDeviceCanvas(game, canvas, 0);
  await aimAtDeviceCanvas(game, canvas, 1);
  await aimAtDeviceCanvas(game, canvas, 0);
  await game.step({ frames: 4 });
}

/** Stand near the entity with a clear view (the given spots, else a ring
 * around it at `standY`), draw the device and scan it. */
async function scanWithDevice(
  game: GameServer,
  template: number,
  standY: number,
  spots?: { x: number; y: number; z: number }[],
  opensPanel = true,
) {
  const [target] = await game.entities.byTemplate(template);
  assert.ok(target, `no entity with template ${template}`);
  const [x, , z] = target.position;
  const ring = [[-1.2, 0], [1.2, 0], [0, -1.2], [0, 1.2], [-1, -1], [1, 1], [-1, 1], [1, -1]];
  let aim;
  for (const spot of spots ?? ring.map(([dx, dz]) => ({ x: x + dx, y: standY, z: z + dz }))) {
    await game.player.teleport(spot);
    await game.step({ frames: 60 });
    try {
      aim = await game.player.aimAt(target.id, { hitbox: "center", visibility: "required" });
      if (aim.target_confirmed) break;
    } catch (error) {
      if (!(error instanceof AimOcclusionError)) throw error;
      aim = undefined;
    }
  }
  assert.ok(aim?.target_confirmed, `no clear view of template ${template}`);
  await drawPersonalCard(game, "left");
  const windowsBefore = (await face(game)).windows.length;
  await aimVrHandAt(game, aim.world_point, 0.05, 1, 0, { hand: "left" });
  await game.step({ frames: 12 });
  const card = (await game.info()).player.hand_feedback!.body_gear!.personal_card;
  assert.equal(card.scans, 1);
  assert.equal(card.on_device, true);
  if (opensPanel) assert.equal((await game.ui.state()).active_panel?.template_id, template);
  assert.equal(card.device_face!.windows.length, windowsBefore + 1, "the scan adds the screen window");
  await raiseDevice(game);
  return target;
}

const center = (r: number[]): [number, number] => [r[0] + r[2] / 2, r[1] + r[3] / 2];

/** Frame the device and its hologram with the debug camera and capture
 * `frames` stills, 0.1 s apart, into `<shots>/<name>/`. */
async function captureHologram(game: GameServer, name: string, frames: number) {
  if (!shots) return;
  mkdirSync(`${shots}/${name}`, { recursive: true });
  // Pointer hand out of shot; let the pawn settle so the frame holds still.
  await game.input.set("right_hand.position", [0.35, 0.3, 0]);
  await game.step({ frames: 90 });
  const f = await face(game);
  const normal = quatRotate(f.rotation, [0, 0, 1]);
  const lookAt = add(f.center, [0, 0.2, 0]);
  await fetch(`${game.baseUrl}/v1/camera`, {
    method: "POST",
    body: JSON.stringify({ position: add(lookAt, add(scale(normal, 0.8), [0, 0.1, 0])), look_at: lookAt }),
  });
  for (let i = 0; i < frames; i++) {
    await game.step({ frames: 6 });
    await game.screenshot(`${shots}/${name}/frame_${String(i).padStart(2, "0")}.png`);
  }
  await fetch(`${game.baseUrl}/v1/camera`, { method: "POST", body: JSON.stringify({ detached: false }) });
  await game.step({ frames: 1 });
}

/** Capture the player's view and a debug-camera close-up of the face. */
async function capture(game: GameServer, name: string) {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  await game.screenshot(`${shots}/${name}-eye.png`);
  const f = await face(game);
  const normal = quatRotate(f.rotation, [0, 0, 1]);
  await fetch(`${game.baseUrl}/v1/camera`, {
    method: "POST",
    body: JSON.stringify({ position: add(f.center, scale(normal, 0.3)), look_at: f.center }),
  });
  await game.step({ frames: 1 });
  await game.screenshot(`${shots}/${name}-close.png`);
  await fetch(`${game.baseUrl}/v1/camera`, { method: "POST", body: JSON.stringify({ detached: false }) });
  await game.step({ frames: 1 });
}

test("VR drawing the device opens use mode on its face; returning closes it", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  assert.equal((await game.ui.state()).mode, "shooter");
  await drawPersonalCard(game, "left");
  await raiseDevice(game);
  assert.equal((await game.ui.state()).mode, "use");
  const card = (await game.info()).player.hand_feedback!.body_gear!.personal_card;
  assert.equal(card.on_device, true);
  // No MFD open: only the bar's windows (two wells, five buttons).
  const barWindows = card.device_face!.windows.length;
  assert.equal(barWindows, 7);
  await capture(game, "device-idle");

  // The bar's MFD button, pressed with the other hand's ray, puts the
  // character sheet on the screen.
  await clickDevice(game, MFD_BUTTON);
  let windows = (await face(game)).windows;
  assert.equal(windows.length, barWindows + 1, "an open MFD adds the screen window");
  assert.deepEqual(windows[0].src, [450, 124, 188, 296]);
  await aimAtDeviceCanvas(game, [540, 200]);
  await capture(game, "device-stats");

  await clickDevice(game, RES_BUTTON);
  windows = (await face(game)).windows;
  assert.deepEqual(windows[0].src, [2, 124, 188, 296]);
  await aimAtDeviceCanvas(game, [90, 250]);
  await capture(game, "device-research");
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  assert.equal((await game.ui.state()).mode, "shooter");
});

test("VR cyber interface takes the canvas over from a held device", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await drawPersonalCard(game, "left");
  await raiseDevice(game);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 3 });
  const card = (await game.info()).player.hand_feedback!.body_gear!.personal_card;
  assert.equal((await game.ui.state()).mode, "use", "use mode survives the handoff");
  assert.equal(card.on_device, false);
  assert.equal(card.hand, null, "the device returns to the belt while the head panel is up");
  // Still squeezing: the held grip must not re-draw the device or reopen it.
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 3 });
  assert.equal((await game.ui.state()).mode, "shooter");
  assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.hand, null);
});

test("VR scanning a keypad with the device opens its panel on the device screen", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await scanWithDevice(game, 266, 21.404);
  await aimAtDeviceCanvas(game, center((await face(game)).windows[0].src));
  await capture(game, "device-keypad");
  await game.input.set("left_hand.squeeze", 0);
  await game.step({ frames: 3 });
  assert.equal((await game.ui.state()).active_panel, null, "returning the device closes the panel");
});

test("VR scanning a crate shows its loot on the device; a squeeze pulls an item into the hand", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  await scanWithDevice(game, 307, 21.404);
  const item = (await game.ui.state()).active_panel!.elements.find((e) => e.entity_id != null);
  assert.ok(item, "the crate panel lists its contents");
  await aimAtDeviceCanvas(game, center(item.rect));
  await capture(game, "device-loot");
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 6 });
  assert.equal((await game.info()).player.right_hand_entity_id, item.entity_id);
  await captureHologram(game, "holo-crate", 24);
});

test("VR scanning a replicator opens its shop on the device without buying", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [rep] = await game.entities.byTemplate(262);
  const [x, , z] = rep.position;
  await scanWithDevice(game, 262, 21.404, [{ x: x - 1.59, y: 21.404, z: z - 2.23 }]);
  const [panel] = (await face(game)).windows;
  // The HRM plug stands right of the 188 px body, over the bar's glow.
  assert.ok(panel.src[2] > 188, "the replicator panel carries its HRM plug");
  await aimAtDeviceCanvas(game, [panel.src[0] + 179 + 36, panel.src[1] + 96 + 97]);
  await capture(game, "device-replicator");
});

test("VR scanning a corpse shows its loot on the device", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "hydro2.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 5 });
  await scanWithDevice(game, 754, -0.76, [{ x: 73.08, y: -0.76, z: -14.92 }]);
  const items = (await game.ui.state()).active_panel!.elements.filter((e) => e.entity_id != null);
  assert.ok(items.length > 0, "the corpse panel lists its contents");
  await aimAtDeviceCanvas(game, center(items[0].rect));
  await capture(game, "device-corpse");
});

test("VR a device scan replaces an open world panel", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [crate] = await game.entities.byTemplate(307);
  const [x, , z] = crate.position;
  await game.player.teleport({ x: x - 1.2, y: 21.404, z });
  await game.step({ frames: 60 });
  const aim = await game.player.aimAt(crate.id, { hitbox: "center", visibility: "required" });
  // Hand frob: the crate's world quad opens.
  await aimVrHandAt(game, aim.world_point);
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 8 });
  const uiBodies = async () =>
    (await game.physics.bodies()).bodies.filter((b) => b.collision_groups.includes("ui")).length;
  assert.ok((await uiBodies()) > 0, "the hand frob opens a world panel");
  await scanWithDevice(game, 307, 21.404, [{ x: x - 1.2, y: 21.404, z }]);
  assert.equal(await uiBodies(), 0, "the device scan closes the world panel");
});

test("VR the scanned object's hologram spins above the device", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [rep] = await game.entities.byTemplate(262);
  await scanWithDevice(game, 262, 21.404, [{ x: rep.position[0] - 1.59, y: 21.404, z: rep.position[2] - 2.23 }]);
  const hologram = async () => {
    const scene = (await (await fetch(`${game.baseUrl}/v1/scene`)).json()) as SceneListResult;
    return scene.objects
      .filter((o) => o.source === "mfd_hologram")
      .map((o) => o.position);
  };
  const before = await hologram();
  assert.ok(before.length > 0, "the scanned replicator's hologram renders");
  await game.step({ frames: 20 });
  const after = await hologram();
  assert.ok(
    after.some((p, i) => p.some((v, k) => Math.abs(v - before[i][k]) > 1e-4)),
    "the hologram spins",
  );
  await captureHologram(game, "holo-replicator", 24);
});

test("VR scanning a loose item shows its description on the device, without picking it up", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [pile] = await game.entities.byTemplate(257);
  assert.ok(pile);
  await scanWithDevice(game, 257, 0, [{ x: pile.position[0] + 0.3, y: pile.position[1] + 0.15, z: pile.position[2] + 0.3 }], false);
  const windows = (await face(game)).windows;
  assert.deepEqual(windows[0].src, [2, 124, 188, 296], "the item's query page fills the screen");
  assert.equal((await game.ui.state()).active_panel, null, "inspecting opens no object panel");
  assert.equal((await game.entities.byTemplate(257)).length, 1, "the item stays in the world");
  assert.equal((await game.info()).player.stats?.nanites, 0, "nothing collected");
  await aimAtDeviceCanvas(game, center(windows[0].src));
  await capture(game, "device-inspect");
  await captureHologram(game, "holo-inspect", 1);
});
