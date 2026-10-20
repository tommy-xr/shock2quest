import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { add, normalize, scale, sub } from "./helpers/vr-hand.js";

// VR: bring a chemical to the specimen held in the other hand. If that
// specimen's research waits on it, the specimen shows "Research" brackets and
// the chemical hand ticks; releasing resumes that project and uses one dose.
// Any other chemical shows nothing and just drops.
const enabled = process.env.SHOCK2_E2E === "1";
const ORGAN = -1095;
const TOXIN = -1341; // asks for Antimony part-way
const ANTIMONY = -145;
const FERMIUM = -20; // not what the toxin asks for
const SPARKLEY_SPHERE = -1944;
const RESEARCH = "Research"; // MISC.STR TechCursor4
const TOOL_READY = { amplitude: 0.45, duration_ms: 60 };
const TOOL_APPLIED = { amplitude: 0.7, duration_ms: 90 };

type Game = GameServer;

const info = async (game: Game) => (await game.info()).player;
const haptics = async (game: Game) => (await info(game)).hand_feedback!.haptics!;
const hints = async (game: Game) => (await game.ui.state()).bracket_hints;

function assertPulse(actual: { amplitude: number; duration_ms: number } | null, expected: typeof TOOL_READY) {
  assert.ok(actual, "a pulse should have been requested");
  assert.ok(Math.abs(actual.amplitude - expected.amplitude) < 1e-4, `amplitude ${actual.amplitude}`);
  assert.equal(actual.duration_ms, expected.duration_ms);
}

async function holdIn(game: Game, hand: "left" | "right", template: number) {
  await game.input.set(`${hand}_hand.squeeze`, 1);
  const { entity_id } = await game.player.spawnItem(template, { hand });
  await game.step({ frames: 3 });
  const player = await info(game);
  // The left-hand slot is reported as the wielded one.
  assert.equal(hand === "left" ? player.wielded_entity_id : player.right_hand_entity_id, entity_id);
  return entity_id;
}

type Frame = { chest: Vec3; left: Vec3 };

/** A point in front of the chest and the player's leftward direction, read
 * off the resting hands before any is placed. */
async function restFrame(game: Game): Promise<Frame> {
  const { left_hand, right_hand } = await game.input.state();
  const [l, r] = [left_hand.world_position!, right_hand.world_position!];
  const player = await info(game);
  const eye = player.position[1] + player.camera_offset[1];
  const half = scale(sub(l, r), 0.5);
  const middle = add(r, half);
  const forward = normalize([middle[0] - player.position[0], 0, middle[2] - player.position[2]]);
  return { chest: add([middle[0], eye - 0.35, middle[2]], scale(forward, 0.15)), left: normalize(half) };
}

/** Grip points in front of the chest: together (within the two-hand reach)
 * or apart. Clear of the shoulder slots, which would claim a release. */
async function placeHands(game: Game, frame: Frame, together: boolean) {
  await game.input.set("left_hand.world_target", add(frame.chest, scale(frame.left, 0.08)));
  await game.input.set("right_hand.world_target", add(frame.chest, scale(frame.left, together ? -0.08 : -0.4)));
  await game.step({ frames: 3 });
}

/** Begin researching the held toxin and run it until it asks for Antimony. */
async function stallOnAntimony(game: Game, toxin: number) {
  await game.entities.sendMessage(toxin, { type: "Frob" });
  await game.step({ frames: 2 });
  for (let frames = 0; frames < 900; frames += 30) {
    await game.step({ frames: 30 });
    if ((await game.ui.state()).messages.at(-1)?.startsWith("Chemical needed")) return;
  }
  assert.fail("research never asked for a chemical");
}

test("VR: the needed chemical, released onto the held specimen, resumes and doses it", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const frame = await restFrame(game);
  const toxin = await holdIn(game, "left", TOXIN);
  await holdIn(game, "right", ANTIMONY);
  await placeHands(game, frame, false);

  // Research not yet at its Antimony gate: an early dose is not taken.
  await game.entities.sendMessage(toxin, { type: "Frob" });
  await game.step({ frames: 2 });
  await placeHands(game, frame, true);
  assert.ok(!(await hints(game)).includes(RESEARCH), "no brackets before the gate");
  await placeHands(game, frame, false);
  await stallOnAntimony(game, toxin);

  // Another project becomes the active one meanwhile.
  const { entity_id: organ } = await game.player.spawnItem(ORGAN);
  await game.entities.sendMessage(organ, { type: "Frob" });
  await game.step({ frames: 2 });
  assert.equal((await info(game)).active_research_template, ORGAN);

  const before = (await haptics(game)).sequence[1];
  await placeHands(game, frame, true);
  assert.deepEqual(await hints(game), [RESEARCH], "the toxin's brackets read the research verb");
  let h = await haptics(game);
  assert.equal(h.sequence[1], before + 1, "the chemical hand ticks once");
  assertPulse(h.last[1], TOOL_READY);
  assert.equal((await info(game)).hand_feedback!.right.affordance, "Frobbable");
  await game.step({ frames: 20 });
  assert.equal((await haptics(game)).sequence[1], before + 1, "lingering does not keep ticking");

  const count = async (template: number) => (await game.entities.byTemplate(template)).length;
  const [nanites, antimonies] = [await count(SPARKLEY_SPHERE), await count(ANTIMONY)];
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 3 });
  const player = await info(game);
  assert.equal(player.active_research_template, TOXIN, "research switches to the toxin");
  assert.equal(player.wielded_entity_id, toxin, "the specimen stays in hand");
  assert.equal(player.right_hand_entity_id, null);
  const { messages } = await game.ui.state();
  assert.equal(messages.at(-1), "Chemical used in research.");
  assert.equal(await count(SPARKLEY_SPHERE), nanites + 1, "it dissolves into nanites");
  assert.equal(await count(ANTIMONY), antimonies, "a stand-in replaces the single dose");
  h = await haptics(game);
  assert.equal(h.sequence[1], before + 2, "the releasing hand confirms once");
  assertPulse(h.last[1], TOOL_APPLIED);
  assert.ok(!(await hints(game)).includes(RESEARCH), "nothing is ready once the dose is used");
  await game.step({ frames: 60 });
  assert.equal(await count(ANTIMONY), antimonies - 1, "the dose is used up");
});

test("VR: a chemical the held specimen does not need shows nothing and just drops", {
  skip: !enabled, timeout: 240_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_research", debugFlags: ["--vr"] });
  await game.step({ frames: 60 });
  const frame = await restFrame(game);
  const toxin = await holdIn(game, "left", TOXIN);
  const fermium = await holdIn(game, "right", FERMIUM);
  await placeHands(game, frame, false);
  await stallOnAntimony(game, toxin);

  const before = (await haptics(game)).sequence[1];
  await placeHands(game, frame, true);
  assert.ok(!(await hints(game)).includes(RESEARCH), "no research brackets");
  assert.equal((await haptics(game)).sequence[1], before, "no ready tick");
  assert.notEqual((await info(game)).hand_feedback!.right.affordance, "Frobbable");

  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 5 });
  assert.equal((await info(game)).right_hand_entity_id, null, "the hand lets go");
  assert.ok(
    (await game.physics.bodies({ entityId: fermium })).bodies.length > 0,
    "the chemical drops into the world",
  );
  assert.ok(!(await game.ui.state()).messages.includes("Chemical used in research."));
  assert.equal((await haptics(game)).sequence[1], before, "the release is silent");
  assert.equal((await info(game)).active_research_template, TOXIN);
});
