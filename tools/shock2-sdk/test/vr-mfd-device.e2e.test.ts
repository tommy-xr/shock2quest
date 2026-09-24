import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
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
  // No MFD open: only the bar's windows (two wells, four buttons).
  const barWindows = card.device_face!.windows.length;
  assert.equal(barWindows, 6);
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
