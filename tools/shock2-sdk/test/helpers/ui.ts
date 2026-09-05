import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";
import type { UiElement, UiPanelPose, UiState } from "../../src/types.js";
import { aimVrHandAtCanvas } from "./vr-hand.js";

/** Click the center of one introspected flat-panel element through real input. */
export async function clickUiElement(
  game: GameServer,
  element: UiElement,
): Promise<void> {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [
    x + width / 2,
    y + height / 2,
  ]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

/** An introspected element's center on the 640x480 canvas. */
export const canvasCenter = (el: UiElement): [number, number] => [
  el.rect[0] + el.rect[2] / 2,
  el.rect[1] + el.rect[3] / 2,
];

/** The VR cyber-interface panel's pose, asserted present. */
export function requirePanelPose(ui: UiState): UiPanelPose {
  assert.ok(ui.panel_pose, "the VR cyber interface must report its panel pose");
  return ui.panel_pose;
}

/**
 * Click a canvas point with a VR controller ray: release, pull, release, so the
 * host sees one clean rising edge wherever the hand was pointing before.
 */
export async function clickCanvasWithRay(
  game: GameServer,
  panel: UiPanelPose,
  canvas: [number, number],
  hand: "left" | "right" = "right",
): Promise<void> {
  for (const trigger of [0, 1, 0]) {
    await aimVrHandAtCanvas(game, panel, canvas, { hand, trigger });
    await game.step({ frames: 3 });
  }
}
