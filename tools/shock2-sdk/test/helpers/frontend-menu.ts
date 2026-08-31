import { PLAYER_EYE_HEIGHT_WORLD } from "../../src/index.js";
import type { GameServer } from "../../src/index.js";

/** The frontend screens are authored on the original 640x480 UI canvas. */
export const CANVAS_W = 640;
export const CANVAS_H = 480;

/** A canvas point normalized to the [0,1] flat `pointer.position` channel. */
export const norm = (x: number, y: number): [number, number] => [x / CANVAS_W, y / CANVAS_H];

/**
 * The normalized center of a main-menu entry. MAINR.BIN stacks six 179x60
 * buttons at x=400 on a 76px pitch, in screen order: 0 "New Game",
 * 1 "Load Game", ... 5 "Quit".
 */
export const menuEntry = (index: number): [number, number] =>
  norm(400 + 179 / 2, 20 + index * 76 + 60 / 2);

/**
 * Click one main-menu entry with the flat pointer. Clicks are rising-edge, so
 * the press has to start on a frame where the previous one was unpressed.
 */
export async function clickMenuEntry(game: GameServer, index: number): Promise<void> {
  await game.input.set("pointer.position", menuEntry(index));
  await game.step({ frames: 5 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 5 });
}

// The VR menu is the same canvas on a world-space panel, driven by a controller
// ray instead of a cursor. The runtime's VR head yaw is +90 degrees (the camera
// looks along -X), so the panel hangs at x=-2 facing back at the head: canvas +x
// maps to world -z (the viewer's right) and canvas +y to -y.
const PANEL_DISTANCE = 2;
const PANEL_SIZE = { x: 2, y: 1.5 };

/** 90-degree yaw: rotates a hand's -Z ray onto the panel's -X. */
export const AIM_AT_PANEL: [number, number, number, number] = [0, 0.7071068, 0, 0.7071068];

/**
 * Pawn-local position of a normalized canvas point on the VR frontend panel. A
 * hand placed here and rotated by `AIM_AT_PANEL` points straight at that point.
 */
export const panelPoint = ([u, v]: [number, number]): [number, number, number] => [
  -PANEL_DISTANCE,
  PLAYER_EYE_HEIGHT_WORLD + (0.5 - v) * PANEL_SIZE.y,
  (0.5 - u) * PANEL_SIZE.x,
];

/** Click a canvas point with the flat pointer, as a real rising edge. */
export async function clickCanvasPoint(
  game: GameServer,
  [x, y]: [number, number],
): Promise<void> {
  await game.input.set("pointer.position", norm(x, y));
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
}

/** Pull the trigger over a canvas point on the VR panel, as a rising edge. */
export async function vrClickCanvasPoint(
  game: GameServer,
  [x, y]: [number, number],
): Promise<void> {
  const [, py, pz] = panelPoint(norm(x, y));
  await game.input.set("right_hand.rotation", AIM_AT_PANEL);
  // Aimed straight down -X: the ray meets the panel at the same canvas point
  // whatever distance the panel is currently tuned to.
  await game.input.set("right_hand.position", [0, py, pz]);
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 3 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 3 });
}
