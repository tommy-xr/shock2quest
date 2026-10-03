import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("VR glove: pickup prompt clears on grab and does not recolour the other hand", {skip: !enabled, timeout: 180_000}, async () => {
  await using game = await GameServer.launch({mission: "debug_interactions", debugFlags: ["--vr"]});
  await game.step({frames: 60});
  const [mug] = await game.entities.byTemplate(-1221);
  assert.ok(mug);
  await game.player.teleport({x: mug.position[0] + 0.6, y: 1, z: 0});
  await game.step({frames: 30});
  await game.input.set("left_hand.position", [0, 2, 0]);
  await game.input.set("left_hand.rotation", [1, 0, 0, 0]);
  await aimVrHandAt(game, mug.position, 0.35);
  let info = await game.info();
  assert.equal(info.player.hand_feedback?.right.target, mug.id);
  assert.equal(info.player.hand_feedback?.right.light, "Green");
  assert.equal(info.player.hand_feedback?.left.light, "Off");
  await game.input.set("right_hand.squeeze", 1);
  await game.step({frames: 3});
  info = await game.info();
  assert.equal(info.player.right_hand_entity_id, mug.id);
  assert.equal(info.player.hand_feedback?.right.light, "Off");
});

test("VR glove: visible production button pulses with a real refused press", {skip: !enabled, timeout: 180_000}, async () => {
  await using game = await GameServer.launch({mission: "debug_interactions", debugFlags: ["--vr"]});
  await game.step({frames: 30});
  const buttons = await game.entities.byTemplate(-201);
  const button = buttons.find(b => b.name === "Feedback locked button");
  const ready = buttons.find(b => b.name === "Feedback ready button");
  assert.ok(button); assert.ok(ready);
  await aimVrHandAt(game, button.position, 0.35);
  const feedback = (await game.info()).player.hand_feedback?.right;
  assert.equal(feedback?.target, button.id);
  assert.equal(feedback?.light, "Amber");
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.input.set("right_hand.trigger", 1);
  await game.step({frames: 1});
  assert.equal((await game.info()).player.hand_feedback?.right.light, "Red");
  await game.step({frames: 16});
  assert.equal((await game.info()).player.hand_feedback?.right.light, "Amber");
  const refusals = (await game.audio.recent()).sounds.filter(s => s.sequence > before && s.sample.toLowerCase() === "hackfail");
  assert.equal(refusals.length, 1, "light pulse must accompany one actual refused press");
  await aimVrHandAt(game, ready.position, 0.35);
  assert.equal((await game.info()).player.hand_feedback?.right.light, "Green");
});

test("VR mission exposes glove feedback through the mission wrapper", {skip: !enabled, timeout: 180_000}, async () => {
  await using game = await GameServer.launch({mission: "eng1.mis", debugFlags: ["--vr"]});
  await game.step({frames: 1});
  assert.ok((await game.info()).player.hand_feedback);
});


test("VR anticipation eases toward available interactions without operating them", {skip: !enabled, timeout: 180_000}, async () => {
  await using game = await GameServer.launch({mission: "debug_interactions", debugFlags: ["--vr"]});
  await game.step({frames: 30});
  const buttons = await game.entities.byTemplate(-201);
  const ready = buttons.find(b => b.name === "Feedback ready button")!;
  const locked = buttons.find(b => b.name === "Feedback locked button")!;
  await aimVrHandAt(game, ready.position, 0.35);
  const early = (await game.info()).player.hand_feedback!.anticipation[1].point;
  assert.ok(early > 0 && early < 0.4, `eases in: ${early}`);
  await game.step({frames: 30});
  let player = (await game.info()).player;
  assert.ok(player.hand_feedback!.anticipation[1].point > 0.6);
  assert.equal(player.right_hand_entity_id, null);
  await aimVrHandAt(game, locked.position, 0.35);
  const leaving = (await game.info()).player.hand_feedback!.anticipation[1].point;
  assert.ok(leaving > 0.2 && leaving < 0.6, `eases out: ${leaving}`);
  await game.step({frames: 60});
  assert.ok((await game.info()).player.hand_feedback!.anticipation[1].point < 0.001);
  const [mug] = await game.entities.byTemplate(-1221);
  await game.player.teleport({x: mug.position[0] + 0.6, y: 1, z: 0});
  await game.step({frames: 30});
  await aimVrHandAt(game, mug.position, 1.0);
  await game.step({frames: 30});
  assert.ok((await game.info()).player.hand_feedback!.anticipation[1].curls.every(c => c < 0.001), "distant pickups do not prepare a grab");
  await aimVrHandAt(game, mug.position, 0.35);
  await game.step({frames: 30});
  player = (await game.info()).player;
  assert.ok(player.hand_feedback!.anticipation[1].curls.some(c => c > 0.1));
  assert.equal(player.right_hand_entity_id, null, "anticipation must not acquire the mug");
  await game.input.set("right_hand.squeeze", 1);
  await game.step({frames: 3});
  player = (await game.info()).player;
  assert.equal(player.right_hand_entity_id, mug.id);
  assert.deepEqual(player.hand_feedback!.anticipation[1].curls, [0, 0, 0, 0, 0]);
});
