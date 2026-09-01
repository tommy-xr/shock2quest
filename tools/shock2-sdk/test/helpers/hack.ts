import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";
import type { UiElement, UiPanel } from "../../src/types.js";
import { clickUiElement } from "./ui.js";

/** Routes tried in turn - each is a straight three-in-a-row on the board. */
const ROUTES = [
  ["node-2-0", "node-3-0", "node-4-0"],
  ["node-2-1", "node-2-2", "node-2-3"],
  ["node-0-1", "node-0-2", "node-0-3"],
  ["node-4-0", "node-4-1", "node-4-2"],
  ["node-0-3", "node-1-3", "node-2-3"],
];

/** The lost board (a mined square resolved against the hacker). */
export const LOST_TEXTURE = "loseh.pcx";
/** The burned-out board - no three-in-a-row is possible any more. */
const UNWINNABLE_TEXTURE = "failh.pcx";
/** The board shown when the wallet cannot cover the authored cost. */
const UNPAID_TEXTURE = "payh.pcx";

export function hasTexture(panel: UiPanel, texture: string): boolean {
  return panel.elements.some(
    (element) => element.texture?.toLowerCase() === texture,
  );
}

export function button(panel: UiPanel, label: string): UiElement {
  const found = panel.elements.find(
    (element) => element.kind === "button" && element.label === label,
  );
  assert.ok(
    found,
    `panel should expose button ${label} (got ${JSON.stringify(
      panel.elements.map((e) => e.label ?? e.texture),
    )})`,
  );
  return found;
}

export async function activePanel(game: GameServer): Promise<UiPanel> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "the interaction should keep an MFD panel open");
  return panel;
}

/** Dismiss whatever panel is open, by clicking the bare view beside it. */
export async function closePanel(game: GameServer): Promise<void> {
  await game.input.set("pointer.position", [0.9, 0.9]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 5 });
}

/**
 * Genuinely play the shared HRM board until `isWon` reports the object gave
 * way: START (charging the authored cost), then light nodes toward a connected
 * three, re-dealing a board that burned itself out. No injected success
 * message and no assumption that any one roll must land.
 */
export async function playHackBoardToWin(
  game: GameServer,
  isWon: (panel: UiPanel) => boolean,
  attempts = 15,
): Promise<UiPanel> {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    let panel = await activePanel(game);
    if (isWon(panel)) return panel;
    // A ruined object is terminal - fail loudly rather than spin.
    assert.ok(
      !hasTexture(panel, LOST_TEXTURE),
      "a critical failure ruined the object; max Hack skill should leave no mines",
    );

    // Deal a board only when one is not already in play - the caller's paid
    // START must not be thrown away by an immediate RESET. A burned-out board
    // is re-dealt with RESET, which charges the authored cost again, exactly
    // as retail does.
    const inPlay = panel.elements.some(
      (element) => element.label === "reset-hack",
    );
    if (!inPlay || hasTexture(panel, UNWINNABLE_TEXTURE)) {
      const deal = panel.elements.find(
        (element) =>
          element.label === "start-hack" || element.label === "reset-hack",
      );
      assert.ok(deal, "an unwon board should offer START/RESET");
      await clickUiElement(game, deal);
      assert.ok(
        !hasTexture(await activePanel(game), UNPAID_TEXTURE),
        "the test wallet should always cover the authored hack cost",
      );
    }

    for (const label of ROUTES[attempt % ROUTES.length]!) {
      panel = await activePanel(game);
      if (isWon(panel)) return panel;
      if (
        hasTexture(panel, UNWINNABLE_TEXTURE) ||
        hasTexture(panel, LOST_TEXTURE)
      ) {
        break;
      }
      await clickUiElement(game, button(panel, label));
    }
  }
  assert.fail(
    `the HRM board should give way within the attempt budget; rng=${game
      .logs()
      .filter((line) => line.includes("HRM rng"))
      .slice(-6)
      .join(" | ")}`,
  );
}

/**
 * Play the board until a mined square resolves against the hacker - the only
 * way a hack fails. Deliberately unskilled: mines stay on the board and every
 * roll is likely to lose.
 */
export async function playHackBoardToCriticalFailure(
  game: GameServer,
  attempts = 25,
): Promise<void> {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    let panel = await activePanel(game);
    if (hasTexture(panel, LOST_TEXTURE)) return;

    const deal = panel.elements.find(
      (element) =>
        element.label === "start-hack" || element.label === "reset-hack",
    );
    const inPlay = panel.elements.some(
      (element) => element.label === "reset-hack",
    );
    if (deal && (!inPlay || hasTexture(panel, UNWINNABLE_TEXTURE))) {
      await clickUiElement(game, deal);
    }

    for (const label of ROUTES[attempt % ROUTES.length]!) {
      panel = await activePanel(game);
      if (hasTexture(panel, LOST_TEXTURE)) return;
      if (hasTexture(panel, UNWINNABLE_TEXTURE)) break;
      const node = panel.elements.find((element) => element.label === label);
      if (!node) break;
      await clickUiElement(game, node);
    }
  }
  assert.fail("an unskilled hacker should have hit a mine by now");
}
