import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { UiState } from "../src/types.js";
import { canvasCenter, clickCanvasWithRay, clickUiElement, requirePanelPose } from "./helpers/ui.js";
import { ammoOf, pullTrigger } from "./helpers/weapon.js";

// An unresearched weapon specimen: using it from the inventory opens its
// Research MFD instead of wielding it; wielded anyway, it refuses to fire
// with retail's "not yet researched" line until research completes.
const enabled = process.env.SHOCK2_E2E === "1";
const WORM_LAUNCHER = -27; // researched with Molybdenum and Selenium
const CHEMICALS = [-979, -136]; // Molybdenum, Selenium
const WORM_BEAKER = -1264; // launcher ammo
const REFUSAL = "This item is not yet researched.";

type Game = GameServer;

const showsResearch = (ui: UiState, entity: number) =>
  ui.active_panel?.entity_id === entity
  && ui.active_panel.elements.some(e => e.texture?.toLowerCase() === "iface/research.pcx");
const ammo = async (game: Game, id: number) => ammoOf(await game.entities.detail(id));
const objectState = async (game: Game, id: number) =>
  (await game.entities.detail(id)).properties.find(p => p.name === "ObjectState")?.value;
const entityCount = async (game: Game) => (await game.entities.list({ limit: 500 })).entities.length;

/** Pull the trigger once; the shot (or refusal) and every new entity it made. */
async function fire(game: Game, weapon: number) {
  const [rounds, entities] = [await ammo(game, weapon), await entityCount(game)];
  await pullTrigger(game);
  await game.step({ frames: 10 });
  const { messages } = await game.ui.state();
  return {
    message: messages.at(-1) ?? "",
    spent: rounds - await ammo(game, weapon),
    spawned: await entityCount(game) - entities,
  };
}

/** Load the wielded launcher from a carried worm beaker. */
async function reload(game: Game, launcher: number) {
  await game.player.spawnItem(WORM_BEAKER);
  await game.input.trigger("Reload");
  await game.step({ frames: 5 });
  for (let i = 0; i < 20 && (await game.info()).player.reloading; i++) {
    await game.step({ frames: 60 });
  }
  assert.ok(await ammo(game, launcher) > 0, "the launcher is loaded");
}

/** Research the carried launcher to completion, offering both of its chemicals
 * as it goes (an unneeded one is refused and kept). */
async function finishResearch(game: Game, launcher: number) {
  const stations = await Promise.all(CHEMICALS.map(async t => (await game.entities.byTemplate(t))[0].id));
  for (const station of stations) await game.player.give(station);
  for (let frames = 0; frames < 20_000; frames += 120) {
    if (await objectState(game, launcher) !== "Unresearched") return;
    for (const station of stations) await game.entities.sendMessage(station, { type: "Frob" });
    await game.step({ frames: 120 });
  }
  assert.fail(`research never completed: ${JSON.stringify((await game.ui.state()).messages)}`);
}

test("flat: using an unresearched weapon researches it; it fires only once researched", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research" });
  await game.step({ frames: 60 });
  const { entity_id: launcher } = await game.player.spawnItem(WORM_LAUNCHER);
  assert.equal(await objectState(game, launcher), "Unresearched");

  // Double-click on the strip: research, not wield.
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });
  const slot = (await game.ui.state()).strip?.elements.find(e => e.entity_id === launcher);
  assert.ok(slot, "the launcher is on the inventory strip");
  await clickUiElement(game, slot);
  await clickUiElement(game, slot);
  await game.step({ frames: 2 });
  assert.ok(showsResearch(await game.ui.state(), launcher), "the Research MFD opens");
  assert.notEqual((await game.info()).player.wielded_entity_id, launcher, "it is not wielded");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 2 });

  // Wielded anyway (the grab path), it refuses to fire.
  await game.player.spawnItem(WORM_LAUNCHER, { hand: "right" }).then(async ({ entity_id }) => {
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.wielded_entity_id, entity_id, "a grab still wields it");
    await reload(game, entity_id);
    const shot = await fire(game, entity_id);
    assert.deepEqual(shot, { message: REFUSAL, spent: 0, spawned: 0 }, "no shot, no ammo spent");
    assert.equal(await objectState(game, entity_id), "Unresearched", "firing starts no research");

    await finishResearch(game, launcher);
    assert.equal(await objectState(game, entity_id), "Normal", "research covers every launcher");
    const researched = await fire(game, entity_id);
    assert.notEqual(researched.message, REFUSAL);
    assert.ok(researched.spent > 0, `a researched launcher fires: ${JSON.stringify(researched)}`);
  });
});

test("VR: cyber-interface use researches an unresearched weapon; its held trigger refuses", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const { entity_id: stowed } = await game.player.spawnItem(WORM_LAUNCHER);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  let ui = await game.ui.state();
  const slot = ui.strip?.elements.find(e => e.kind === "button" && e.entity_id === stowed);
  assert.ok(slot, "the launcher is on the interface's inventory strip");
  await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(slot));
  ui = await game.ui.state();
  assert.ok(showsResearch(ui, stowed), `the interface MFD shows its research: ${JSON.stringify(ui.active_panel?.entity_id)}`);
  assert.equal((await game.info()).player.right_hand_entity_id, null, "nothing is wielded");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });

  await game.input.set("right_hand.squeeze", 1);
  const { entity_id: held } = await game.player.spawnItem(WORM_LAUNCHER, { hand: "right" });
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.right_hand_entity_id, held, "a grab still wields it");
  await reload(game, held);
  const shot = await fire(game, held);
  assert.deepEqual(shot, { message: REFUSAL, spent: 0, spawned: 0 }, "no shot, no ammo spent");
});
