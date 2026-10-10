import assert from "node:assert/strict";
import type { GameServer, UiElement } from "../../src/index.js";
import { clickElement } from "./os-upgrade.js";
import { canvasCenter } from "./ui.js";

/** Node art stays centered inside the more generous invisible click target. */
export function nodeOverlay(elements: UiElement[], node: UiElement): UiElement | undefined {
  const [x, y] = canvasCenter(node);
  return elements.find(element => {
    if (!/^hrm(mine|burn|on)\.pcx$/i.test(element.texture ?? "")) return false;
    const [artX, artY] = canvasCenter(element);
    return Math.abs(artX - x) < .01 && Math.abs(artY - y) < .01;
  });
}

/** The open panel's elements. */
export async function elements(game: GameServer) {
  return (await game.ui.state()).active_panel!.elements;
}

/** Open the wielded gun's settings panel from the use-mode ammo readout. */
export async function openSettings(game: GameServer) {
  if ((await game.ui.state()).mode !== "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
  }
  const button = (await game.ui.state()).readout.find((e) => e.label === "gun_setting");
  assert.ok(button, "the ammo readout offers SETTING");
  await clickElement(game, button);
}

/** Leave use mode, closing the MFD. */
export async function closeSettings(game: GameServer) {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });
}

/** Retail's HRM board has no way back: close the MFD and reopen settings. */
export async function reopenSettings(game: GameServer) {
  await closeSettings(game);
  await openSettings(game);
}

export async function property(game: GameServer, entity: number, name: string) {
  return (await game.entities.detail(entity)).properties.find((p) => p.name === name)!.value;
}

/** Play paid HRM boards through the real UI until one is won. Avoids known
 * mines; a burned board is re-dealt, each re-deal another honest payment. */
export async function winBoard(game: GameServer) {
  const won = async () => (await elements(game)).some((e) => /win[hmr]\.pcx/.test(e.texture ?? ""));
  for (let attempt = 0; attempt < 20; attempt++) {
    const start = (await elements(game)).find(
      (e) => e.label === "start-hack" || e.label === "reset-hack",
    );
    assert.ok(start, "a paid attempt remains available");
    await clickElement(game, start);
    for (const node of (await elements(game)).filter((e) => e.label?.startsWith("node-"))) {
      const current = await elements(game);
      if (current.some((e) => /(win|fail)[hmr]\.pcx/.test(e.texture ?? ""))) break;
      const overlay = nodeOverlay(current, node);
      if (!overlay) await clickElement(game, node);
    }
    if (await won()) return;
  }
  assert.fail("20 paid HRM attempts did not win");
}
