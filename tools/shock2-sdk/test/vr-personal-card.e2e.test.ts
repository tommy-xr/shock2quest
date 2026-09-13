import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, drawPersonalCard } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("VR nanites credit on release exactly once, with sound and releasing-hand haptics", {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [pile] = await game.entities.byTemplate(257);
  assert.ok(pile);
  const [x, y, z] = pile.position;
  await game.player.teleport({ x: x + .3, y: y + .15, z: z + .3 });
  await game.step({ frames: 120 });
  const aim = await game.player.aimAt(pile.id, { hitbox: "center", visibility: "required" });
  assert.ok(aim.target_confirmed);
  await aimVrHandAt(game, aim.world_point, .35, 0);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 12 });
  let player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, pile.id);
  assert.equal(player.stats?.nanites, 0);
  const pulses = player.hand_feedback?.haptics?.sequence[1] ?? 0;
  const since = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.position", [.3, .9, -.55]);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 12 });
  player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, null);
  assert.equal(player.stats?.nanites, 250);
  assert.equal(player.hand_feedback?.haptics?.sequence[1], pulses + 1);
  assert.equal((await game.entities.byTemplate(257)).length, 0);
  const audio = (await game.audio.recent()).sounds.filter(s => s.sequence > since);
  assert.ok(audio.length > 0, "collection must emit an audible cue");
  await game.step({ frames: 60 });
  assert.equal((await game.info()).player.stats?.nanites, 250);
});

for (const hand of ["left", "right"] as const) {
  test(`VR ${hand} personal card authorizes a replicator, consumes no nanites, returns on release`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const [reader] = await game.entities.byTemplate(262);
    assert.ok(reader);
    const [x, , z] = reader.position;
    await game.player.teleport({ x: x - 1.59, y: 21.404, z: z - 2.23 });
    await game.step({ frames: 120 });
    const aim = await game.player.aimAt(reader.id, { hitbox: "center", visibility: "required" });
    assert.ok(aim.target_confirmed);
    await aimVrHandAt(game, aim.world_point, .2, 1, 0, { hand });
    await game.step({ frames: 8 });
    assert.equal((await game.ui.state()).active_panel, null);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await drawPersonalCard(game, hand);
    await aimVrHandAt(game, aim.world_point, .12, 1, 0, { hand });
    await game.step({ frames: 12 });
    assert.equal((await game.ui.state()).active_panel?.template_id, 262);
    let card = (await game.info()).player.hand_feedback!.body_gear!.personal_card;
    assert.equal(card.scans, 1);
    await game.step({ frames: 36 });
    assert.equal((await game.info()).player.hand_feedback!.body_gear!.personal_card.scans, 1);
    assert.equal((await game.info()).player.stats?.nanites, 0, "scan must not buy anything");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 3 });
    card = (await game.info()).player.hand_feedback!.body_gear!.personal_card;
    assert.equal(card.hand, null);
  });
}
