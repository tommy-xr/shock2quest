import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { setHandWorldPose } from "../src/vr-pose.js";
import { clickUiElement } from "./helpers/ui.js";
import { aimVrHandAt, drawPersonalCard } from "./helpers/vr-hand.js";

const options = { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 };
const cues = async (game: GameServer) => (await game.audio.recent()).sounds
  .filter(s => ["pickup", "placeinv", "bb11"].includes(s.sample)).map(s => s.sample);

async function fillPack(game: GameServer) {
  // This max-stat debug scene exposes all 45 cells; each mug occupies one.
  while ((await game.player.inventory()).items.filter(i => i.location === "inventory").length < 45)
    await game.player.spawnItem(-1221);
}

async function flatPickup(game: GameServer, template: number) {
  const carried = new Set((await game.player.inventory()).items.map(i => i.entity_id));
  const item = (await game.entities.byTemplate(template)).find(e => !carried.has(e.id));
  assert.ok(item);
  await game.player.teleport({ x: item.position[0] + 1, y: item.position[1], z: item.position[2] + 1 });
  await game.player.aimAt(item.id, { hitbox: "center", visibility: "required" });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 90 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 2 });
  return item;
}

async function reachStorage(game: GameServer, kind: "holsters" | "shoulder_backpack") {
  const player = (await game.info()).player;
  const center = player.hand_feedback?.[kind]?.centers?.[1];
  assert.ok(center);
  await setHandWorldPose(game, player, "right", center, player.rotation);
  await game.step({ frames: 5 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
}

test("flat inventory cues follow pickup, placement, rejection and full-pack stack merging", options, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions" });
  await game.step({ frames: 30 });
  await game.player.spawnItem(-1358);
  assert.deepEqual(await cues(game), [], "debug setup is silent");
  const mug = await flatPickup(game, -1221);
  assert.deepEqual(await cues(game), ["pickup"], "holding pickup does not replay the cue");
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 3 });
  const slot = (await game.ui.state()).strip?.elements.find(e => e.entity_id === mug.id);
  assert.ok(slot);
  await clickUiElement(game, slot);
  await game.step({ frames: 25 }); // place, rather than double-click to use
  await clickUiElement(game, slot);
  assert.deepEqual(await cues(game), ["pickup", "placeinv"]);
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 3 });
  await fillPack(game);
  const refused = await flatPickup(game, -1255);
  assert.ok(!(await game.player.inventory()).items.some(i => i.entity_id === refused.id));
  assert.deepEqual(await cues(game), ["pickup", "placeinv", "bb11"]);
  await flatPickup(game, -1358);
  assert.deepEqual(await cues(game), ["pickup", "placeinv", "bb11", "pickup"], "a successful merge is not an inventory-full failure");
});

test("VR world pickup, shoulder deposit and holster deposit each sound once", options, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  for (const [template, kind] of [[-1221, "shoulder_backpack"], [-17, "holsters"]] as const) {
    const [item] = await game.entities.byTemplate(template);
    await aimVrHandAt(game, item.position, .2, 1);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, item.id);
    await reachStorage(game, kind);
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    await game.step({ frames: 90 });
  }
  assert.deepEqual(await cues(game), ["pickup", "placeinv", "pickup", "placeinv"]);
});

test("a rejected shoulder deposit announces once and never plays placement", options, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [mug] = await game.entities.byTemplate(-1221);
  await aimVrHandAt(game, mug.position, .2, 1);
  await game.step({ frames: 5 });
  await fillPack(game);
  await reachStorage(game, "shoulder_backpack");
  await game.step({ frames: 120 });
  assert.equal((await game.info()).player.right_hand_entity_id, mug.id);
  assert.deepEqual(await cues(game), ["pickup", "bb11"]);
});

test("returning the MFD to the belt confirms placement once", options, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr", "--experimental", "mfd_device"] });
  await game.step({ frames: 30 });
  await drawPersonalCard(game, "right");
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 120 });
  assert.equal((await game.info()).player.hand_feedback?.body_gear?.personal_card.hand, null);
  assert.deepEqual(await cues(game), ["placeinv"]);
});
