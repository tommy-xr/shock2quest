import assert from "node:assert/strict";
import type { GameServer } from "../../src/index.js";
import type { UiElement } from "../../src/types.js";
import { teleportVerified } from "./teleport.js";

/** Find the level's Trait Machine by stable mission object id. */
export async function findMachine(game: GameServer, missionId: number) {
  const entities = (await game.entities.list({ filter: "Trait Machine" })).entities;
  const machine = entities.find((e) => e.template_id === missionId);
  assert.ok(machine, `the level should contain Trait Machine (mission id ${missionId})`);
  return machine;
}

/** Teleport next to an entity, probing a few offsets for stable footing
 * (some machines sit next to pits; a bad offset leaves the player falling
 * out of the panel's walk-away radius). */
export async function standNear(game: GameServer, id: number) {
  const detail = await game.entities.detail(id);
  const [x, y, z] = detail.position;
  for (const [dx, dz] of [
    [0, 1.2],
    [0, -1.2],
    [1.2, 0],
    [-1.2, 0],
  ]) {
    await teleportVerified(game, { x: x + dx, y: y + 0.5, z: z + dz });
    await game.step({ frames: 30 });
    const p = (await game.info()).player.position;
    const dist = Math.hypot(p[0] - x, p[1] - y, p[2] - z);
    if (dist < 3.0) return;
  }
  assert.fail("could not find stable footing near the trait machine");
}

export async function clickElement(game: GameServer, el: UiElement) {
  const [x, y, w, h] = el.screen_rect;
  await game.input.set("pointer.position", [x + w / 2, y + h / 2]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}

export const traitButton = (label: string, els: UiElement[]) => {
  const el = els.find((e) => e.kind === "button" && e.label === label);
  assert.ok(el, `panel should expose a trait button labeled "${label}"`);
  return el;
};

/** Acquire through the real, single-use machine UI; never inject a trait. */
export async function acquireOsUpgrade(game: GameServer, name: string, machineId = 133) {
  const machine = await findMachine(game, machineId);
  await standNear(game, machine.id);
  await game.entities.sendMessage(machine.id, { type: "Frob" });
  await game.step({ frames: 5 });
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel);
  const before = (await game.info()).player.stats!.os_traits.length;
  await clickElement(game, traitButton(name, panel.elements));
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.stats!.os_traits.length, before + 1, `${name} is acquired`);
  if ((await game.ui.state()).mode === "use") {
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 2 });
  }
}
