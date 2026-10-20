import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";
import { drawPersonalCard } from "./helpers/vr-hand.js";

// In VR the Research MFD shows on the drawn tricorder or in the cyber
// interface, never as a world quad on a carried specimen (#2379). With
// neither up, a held specimen's trigger researches and reports on the HUD.
const enabled = process.env.SHOCK2_E2E === "1";
const ORGAN = -1095;
const TOXIN = -1341; // asks for Antimony part-way

type Game = GameServer;

const uiBodies = async (game: Game) =>
  (await game.physics.bodies()).bodies.filter(b => b.collision_groups.includes("ui")).length;

async function hold(game: Game, template: number): Promise<number> {
  await game.input.set("right_hand.squeeze", 1);
  const { entity_id } = await game.player.spawnItem(template, { hand: "right" });
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.right_hand_entity_id, entity_id);
  return entity_id;
}

/** Pull the holding hand's trigger; return the newest HUD line. */
async function pull(game: Game): Promise<string> {
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  const ui = await game.ui.state();
  assert.equal(ui.active_panel, null, "no Research panel opens on the hand");
  assert.equal(await uiBodies(game), 0, "no world-space panel quad");
  return ui.messages.at(-1) ?? "";
}

/** Step until the newest sound after `since` is `sample`. */
async function stepUntilSound(game: Game, sample: string, maxFrames: number) {
  const since = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  for (let frames = 0; frames < maxFrames; frames += 30) {
    await game.step({ frames: 30 });
    if ((await game.audio.recent()).sounds.some(s => s.sequence > since && s.sample === sample)) return;
  }
  assert.fail(`no ${sample} within ${maxFrames} frames`);
}

const percent = (line: string) => Number(/: (\d+\.\d) %$/.exec(line)?.[1]);

test("VR held specimen trigger researches and reports progress on the HUD", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  await hold(game, ORGAN);
  const first = await pull(game);
  assert.match(first, /^Researching .+: \d+\.\d %$/);
  await game.step({ frames: 60 });
  const later = await pull(game);
  assert.ok(percent(later) > percent(first), `research advances: ${first} -> ${later}`);
  await stepUntilSound(game, "bb06", 600);
  assert.match(await pull(game), /: Research complete$/);
});

test("VR held specimen trigger reports the chemical a stalled project needs", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  await hold(game, TOXIN);
  assert.match(await pull(game), /^Researching /);
  await stepUntilSound(game, "bb05", 900);
  assert.equal(await pull(game), "Research paused. Required chemical: Antimony (Sb).");
});

test("VR held specimen trigger reports a Research skill that is too low", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  assert.equal((await game.info()).player.stats?.skills.research, 0);
  await hold(game, ORGAN);
  assert.match(await pull(game), /^This item requires a Research skill of \d+\./);
});

test("VR cyber interface use shows the Research MFD in the interface", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const { entity_id } = await game.player.spawnItem(ORGAN);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  let ui = await game.ui.state();
  const slot = ui.strip?.elements.find(e => e.kind === "button" && e.entity_id === entity_id);
  assert.ok(slot, "the specimen is on the interface's inventory strip");
  await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(slot));
  await clickCanvasWithRay(game, requirePanelPose(ui), canvasCenter(slot));
  ui = await game.ui.state();
  assert.equal(ui.mode, "use", "the interface stays open");
  assert.equal(ui.active_panel?.entity_id, entity_id);
  assert.ok(ui.active_panel.elements.some(e => e.texture?.toLowerCase() === "iface/research.pcx"),
    "the interface MFD shows the Research GUI");
  assert.equal(await uiBodies(game), 0, "no world-space panel quad");
});

test("VR held specimen trigger with the tricorder drawn shows research on its screen", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const organ = await hold(game, ORGAN);
  await drawPersonalCard(game, "left");
  await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 2 });
  const ui = await game.ui.state();
  assert.equal(ui.active_panel?.entity_id, organ);
  assert.ok(ui.active_panel.elements.some(e => e.texture?.toLowerCase() === "iface/research.pcx"));
  assert.equal(await uiBodies(game), 0, "no world-space panel quad");
});
