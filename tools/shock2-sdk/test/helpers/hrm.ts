import assert from "node:assert/strict";
import type { GameServer } from "../../src/index.js";
import { clickElement } from "./os-upgrade.js";

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
  const won = async () => (await elements(game)).some((e) => /win[hm]\.pcx/.test(e.texture ?? ""));
  for (let attempt = 0; attempt < 20; attempt++) {
    const start = (await elements(game)).find(
      (e) => e.label === "start-hack" || e.label === "reset-hack",
    );
    assert.ok(start, "a paid attempt remains available");
    await clickElement(game, start);
    for (const node of (await elements(game)).filter((e) => e.label?.startsWith("node-"))) {
      const current = await elements(game);
      if (current.some((e) => /(win|fail)[hm]\.pcx/.test(e.texture ?? ""))) break;
      const overlay = current.find(
        (e) => e.kind === "image" && e.rect[0] === node.rect[0] && e.rect[1] === node.rect[1],
      );
      if (!overlay) await clickElement(game, node);
    }
    if (await won()) return;
  }
  assert.fail("20 paid HRM attempts did not win");
}
