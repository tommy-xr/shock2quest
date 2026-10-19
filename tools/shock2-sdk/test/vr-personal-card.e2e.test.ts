import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { crossEarthTrainingTripwire } from "./helpers/earth-tripwire.js";
import { add, sub, scale, normalize, quatRotate, quatConjugate, aimVrHandAt, drawPersonalCard } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const collect of ["release", "trigger"] as const) test(`VR nanites credit on ${collect} exactly once, with sound, haptics and sparks`, {
  skip: !enabled, timeout: 180_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const [pile] = await game.entities.byTemplate(257);
  assert.ok(pile);
  const [x, y, z] = pile.position;
  // Keep the reaching palm clear of shoulder-backpack gesture arbitration.
  await game.player.teleport({ x: x + .8, y: y + 1, z: z + .8 });
  await game.step({ frames: 120 });
  const aim = await game.player.aimAt(pile.id, { hitbox: "center", visibility: "required" });
  assert.ok(aim.target_confirmed);
  await aimVrHandAt(game, aim.world_point, .35, 0);
  assert.equal((await game.info()).player.hand_feedback?.shoulder_backpack?.near[1], false,
    "world pickup must start outside the backpack gesture region");
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 12 });
  let player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, pile.id);
  assert.equal(player.stats?.nanites, 0);
  const pulses = player.hand_feedback?.haptics?.sequence[1] ?? 0;
  const since = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  const sparks = async () => (await game.entities.byTemplate(-1944)).length; // sparkley sphere
  const idleSparks = await sparks();
  await game.input.set("right_hand.position", [.3, .9, -.55]);
  if (collect === "release") await game.input.set("right_hand.squeeze", 0);
  else await game.input.set("right_hand.trigger", 1);
  await game.step({ frames: 12 });
  player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, null);
  assert.equal(player.stats?.nanites, 250);
  assert.equal(player.hand_feedback?.haptics?.sequence[1], pulses + 1);
  assert.equal(await sparks(), idleSparks + 1, "the pile breaks down into sparks");
  const audio = (await game.audio.recent()).sounds.filter(s => s.sequence > since);
  assert.ok(audio.length > 0, "collection must emit an audible cue");
  await game.step({ frames: 60 });
  assert.equal((await game.entities.byTemplate(257)).length, 0, "its shrinking stand-in is gone");
  assert.equal(await sparks(), idleSparks);
  assert.equal((await game.info()).player.stats?.nanites, 250);
});

for (const hand of ["left", "right"] as const) {
  test(`VR ${hand} personal card authorizes a replicator, consumes no nanites, returns on release`, {
    skip: !enabled, timeout: 180_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "earth.mis", port: 0, debugFlags: ["--vr"] });
    await game.devParams.set("vr_mfd_device", 0); // Exercise the legacy card toggle.
    await game.step({ frames: 30 });
    await crossEarthTrainingTripwire(game, 313, 319);
    const [reader] = await game.entities.byTemplate(262);
    assert.ok(reader);
    const [x, , z] = reader.position;
    await game.player.teleport({ x: x - 1.59, y: 21.404, z: z - 2.23 });
    await game.step({ frames: 120 });
    const aim = await game.player.aimAt(reader.id, { hitbox: "center", visibility: "required" });
    assert.ok(aim.target_confirmed);
    await aimVrHandAt(game, aim.world_point, .2, 0, 0, { hand });
    await game.step({ frames: 8 });
    assert.equal((await game.ui.state()).active_panel, null);
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await drawPersonalCard(game, hand);
    const contact = await aimVrHandAt(game, aim.world_point, .12, 1, 0, { hand });
    // Hand input is the raw aim pose. Compensate the default -15 cm glove
    // calibration so the calibrated hand, and its offset card, reach the reader.
    const rotation = (await game.info()).player.rotation;
    const forward = quatRotate(quatConjugate(rotation), normalize(sub(contact.target, contact.start)));
    await game.input.set(`${hand}_hand.position`, add(contact.local, scale(forward, .15 / .762)));
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
