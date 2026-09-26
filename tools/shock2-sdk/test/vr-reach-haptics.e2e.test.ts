import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt, quatConjugate, quatRotate, sub } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const hand of ["left", "right"] as const) {
  test(`${hand} shoulder cues once on approach, stows beside head, and cues recall`, { skip: !enabled, timeout: 180_000 }, async () => {
    await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
    await game.step({ frames: 30 });
    const i = hand === "left" ? 0 : 1;
    const owner = hand === "left" ? "wielded_entity_id" : "right_hand_entity_id";
    const pulses = async () => (await game.info()).player.hand_feedback!.haptics!.sequence;
    const item = (await game.entities.list()).entities.find(e => e.template_id === -19)!;
    const grab = await aimVrHandAt(game, item.position, 0.2, 1, 0, { hand });
    await game.step({ frames: 5 });
    let player = (await game.info()).player;
    assert.equal(player[owner], item.id);
    const palmTarget = quatRotate(quatConjugate(player.rotation), sub(player.hand_feedback!.shoulder_backpack!.centers![i], player.position));
    // Above and slightly forward of the new center: reachable beside the head,
    // but outside the old low/behind shoulder sphere.
    palmTarget[1] += 0.15 / 0.762;
    palmTarget[2] -= 0.08 / 0.762;
    const palmNow = quatRotate(quatConjugate(player.rotation), sub(player.hand_feedback!.glove_contacts!.centers[i]!, player.position));
    const target = grab.local.map((v, j) => v + palmTarget[j] - palmNow[j]);
    const before = await pulses();
    await game.input.set(`${hand}_hand.position`, target);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.hand_feedback!.shoulder_backpack!.near[i], true);
    assert.equal((await pulses())[i], before[i] + 1);
    assert.equal((await pulses())[1 - i], before[1 - i]);
    await game.step({ frames: 60 });
    assert.equal((await pulses())[i], before[i] + 1, "lingering does not keep buzzing");
    await game.input.set(`${hand}_hand.squeeze`, 0);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[owner], null);
    assert.equal((await game.info()).player.hand_feedback!.body_gear!.shoulder_weapons![i], item.id);
    await game.input.set(`${hand}_hand.position`, [i === 0 ? -0.8 : 0.8, 0.8, -0.8]);
    await game.step({ frames: 15 });
    await game.input.set(`${hand}_hand.position`, target);
    await game.step({ frames: 3 });
    assert.equal((await pulses())[i], before[i] + 2, "a remembered weapon cues empty-hand recall");
    await game.input.set(`${hand}_hand.squeeze`, 1);
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player[owner], item.id);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 10 });
    assert.equal((await pulses())[i], before[i] + 2, "disabled body gestures cannot cue");
    assert.deepEqual((await game.info()).player.hand_feedback!.haptics!.pending, [null, null]);
  });
}

test("enlarged holster accepts a release thirty centimetres below its mesh", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  await game.step({ frames: 30 });
  const item = (await game.entities.list()).entities.find(e => e.template_id === -928)!;
  const grab = await aimVrHandAt(game, item.position, 0.2, 1);
  await game.step({ frames: 5 });
  const player = (await game.info()).player;
  const palmTarget = quatRotate(quatConjugate(player.rotation), sub(player.hand_feedback!.holsters!.centers![0], player.position));
  palmTarget[1] -= 0.30 / 0.762;
  const palmNow = quatRotate(quatConjugate(player.rotation), sub(player.hand_feedback!.glove_contacts!.centers[1]!, player.position));
  const target = grab.local.map((v, j) => v + palmTarget[j] - palmNow[j]);
  await game.input.set("right_hand.position", target);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.near[1], 0);
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.items[0], item.id);
  assert.equal((await game.info()).player.right_hand_entity_id, null);
  // Radius can be tuned live without moving the mesh or its center.
  await game.devParams.set("vr_holster_radius", 0.14);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.near[1], null);
});

test("glove sphere contacts a holster while its calibrated palm is still outside", { skip: !enabled, timeout: 180_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_interactions", debugFlags: ["--vr"] });
  const start: [number, number, number] = [0.8, 1, -0.4];
  await game.input.set("right_hand.position", start);
  await game.input.set("right_hand.rotation", [0, 0, 0, 1]);
  await game.step({ frames: 30 });
  let player = (await game.info()).player;
  const contacts = player.hand_feedback!.glove_contacts!;
  const holster = player.hand_feedback!.holsters!;
  assert.ok(contacts.centers[1]);
  const target: [number, number, number] = [...holster.centers![0]];
  target[1] -= holster.radius + contacts.radius * 0.5;
  const shift = quatRotate(quatConjugate(player.rotation), sub(target, contacts.centers[1]));
  await game.input.set("right_hand.position", start.map((v, i) => v + shift[i]));
  await game.step({ frames: 3 });
  player = (await game.info()).player;
  const distance = Math.hypot(...sub(player.hand_feedback!.glove_contacts!.centers[1]!, player.hand_feedback!.holsters!.centers![0]));
  assert.ok(distance > holster.radius, "palm center has not entered the holster region");
  assert.ok(distance < holster.radius + contacts.radius, "the two sphere surfaces overlap");
  assert.equal(player.hand_feedback!.holsters!.near[1], 0);
  await game.devParams.set("vr_glove_spheres", 1);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.near[1], 0, "visualization does not change contact");
  await game.devParams.set("vr_glove_radius", 0.02);
  await game.step({ frames: 3 });
  assert.equal((await game.info()).player.hand_feedback!.holsters!.near[1], null, "radius is live and uses surface contact");
});
