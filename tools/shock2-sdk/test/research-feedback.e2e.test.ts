import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

// Research reports chemicals and skill gates on the HUD, flat and VR alike;
// a chemical used up from a VR hand dissolves into nanites with a pulse.
const enabled = process.env.SHOCK2_E2E === "1";
const ORGAN = -1095;
const TOXIN = -1341; // asks for Antimony part-way
const ANTIMONY = -145;
const SPARKLEY_SPHERE = -1944;

type Game = GameServer;

const lastMessage = async (game: Game) => (await game.ui.state()).messages.at(-1) ?? "";
const stack = async (game: Game, id: number) =>
  Number((await game.entities.detail(id)).properties.find(p => p.name === "StackCount")?.value ?? 1);

/** Carry the toxin and research it until it stalls on Antimony. */
async function stallOnAntimony(game: Game) {
  const { entity_id } = await game.player.spawnItem(TOXIN);
  await game.entities.sendMessage(entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  for (let frames = 0; frames < 900; frames += 30) {
    await game.step({ frames: 30 });
    if ((await lastMessage(game)).startsWith("Chemical needed")) return;
  }
  assert.fail("research never asked for a chemical");
}

/** The lab's chemical station for `template`, carried. */
async function carryStation(game: Game, template: number) {
  const [station] = await game.entities.byTemplate(template);
  await game.player.give(station.id);
  return station.id;
}

for (const vr of [false, true]) test(`chemical feedback on the HUD (${vr ? "VR" : "flat"})`, {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: vr ? ["--vr"] : [] });
  await game.step({ frames: 60 });
  await stallOnAntimony(game);
  assert.equal(await lastMessage(game), "Chemical needed: Antimony (Sb)");

  const wrong = await carryStation(game, -20);
  const wrongDoses = await stack(game, wrong);
  await game.entities.sendMessage(wrong, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal(await lastMessage(game), "Please insert correct chemical.");
  assert.equal(await stack(game, wrong), wrongDoses, "a wrong chemical is left alone");

  const antimony = await carryStation(game, ANTIMONY);
  const doses = await stack(game, antimony);
  await game.entities.sendMessage(antimony, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal(await lastMessage(game), "Chemical used in research.");
  assert.equal(await stack(game, antimony), doses - 1, "one dose is consumed");
});

test("flat research with too little skill says so on the HUD", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_minimal" });
  await game.step({ frames: 30 });
  assert.equal((await game.info()).player.stats?.skills.research, 0);
  const { entity_id } = await game.player.spawnItem(ORGAN);
  await game.entities.sendMessage(entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal((await game.ui.state()).active_panel?.entity_id, entity_id, "the Research panel opens");
  assert.match(await lastMessage(game), /^This item requires a Research skill of \d+ to be researched\.$/);
});

test("VR chemical used from a hand dissolves into nanites with a pulse", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  await stallOnAntimony(game);
  await game.input.set("right_hand.squeeze", 1);
  const { entity_id } = await game.player.spawnItem(ANTIMONY, { hand: "right" });
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.right_hand_entity_id, entity_id);
  assert.equal(await stack(game, entity_id), 1, "a single dose");
  const nanites = async () => (await game.entities.byTemplate(SPARKLEY_SPHERE)).length;
  const antimonies = async () => (await game.entities.byTemplate(ANTIMONY)).length;
  const [idleNanites, idleAntimony] = [await nanites(), await antimonies()];
  const pulses = (await game.info()).player.hand_feedback!.haptics!.sequence[1];

  await pullTrigger(game);
  await game.step({ frames: 2 });
  assert.equal(await lastMessage(game), "Chemical used in research.");
  assert.equal((await game.info()).player.right_hand_entity_id, null, "the dose is used up");
  assert.equal(await nanites(), idleNanites + 1, "nanite sparks burst off it");
  assert.equal(await antimonies(), idleAntimony, "a stand-in replaces it");
  const haptics = (await game.info()).player.hand_feedback!.haptics!;
  assert.ok(haptics.sequence[1] > pulses, "the holding hand pulses");
  assert.ok(Math.abs(haptics.last[1]!.amplitude - 0.7) < 1e-6 && haptics.last[1]!.duration_ms === 90,
    `a firm "tool applied" pulse: ${JSON.stringify(haptics.last[1])}`);
  await game.step({ frames: 60 });
  assert.equal(await antimonies(), idleAntimony - 1, "the stand-in shrinks away");
});
