import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";
import type { Vec3 } from "../src/types.js";
import { add, aimVrHandAt, scale, sub } from "./helpers/vr-hand.js";

// A held tool cues the hand before release when its target will accept it
// (#817 follow-up): one TOOL_READY pulse on entering the crate, none while
// lingering, again on re-entry, then a stronger TOOL_APPLIED pulse when the
// release is consumed. Items the crate refuses never cue.
const enabled = process.env.SHOCK2_E2E === "1";

const SECURITY_CRATE = 325; // hydro1, still hackable
const ICE_PICK = -73;
const TOOL_READY = { amplitude: 0.45, duration_ms: 60 };
const TOOL_APPLIED = { amplitude: 0.7, duration_ms: 90 };

async function setup(game: GameServer, item: string | number) {
  await game.step({ frames: 5 });
  const [crate] = await game.entities.byTemplate(SECURITY_CRATE);
  assert.ok(crate, "hydro1 should contain security crate 325");
  const cratePos = (await game.entities.detail(crate.id)).position as Vec3;
  await teleportVerified(game, { x: cratePos[0] + 1.2, y: cratePos[1] + 0.5, z: cratePos[2] + 1.2 });
  await game.step({ frames: 3 });
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 2 });
  const spawned = await game.player.spawnItem(item, { hand: "right" });
  await game.step({ frames: 5 });
  const held = (await game.info()).player.right_hand_entity_id;
  assert.ok(held !== null, `the right hand should hold the spawned ${item} (${JSON.stringify(spawned)})`);
  // Away from the crate: directly behind the player.
  const player = (await game.info()).player.position as Vec3;
  const away = add(player, scale(sub(player, cratePos), 2));
  await aimVrHandAt(game, away, 0.45, 1);
  await game.step({ frames: 5 });
  return { cratePos, held, away };
}

/** Pulses travel as f32, so compare amplitude loosely. */
function assertPulse(actual: { amplitude: number; duration_ms: number } | null, expected: typeof TOOL_READY) {
  assert.ok(actual, "a pulse should have been requested");
  assert.ok(Math.abs(actual.amplitude - expected.amplitude) < 1e-4, `amplitude ${actual.amplitude}`);
  assert.equal(actual.duration_ms, expected.duration_ms);
}

async function haptics(game: GameServer) {
  return (await game.info()).player.hand_feedback!.haptics!;
}

test(
  "a held ICE Pick cues once over a hackable crate and confirms when consumed",
  { skip: !enabled, timeout: 240_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: ["--vr"] });
    const { cratePos, held, away } = await setup(game, ICE_PICK);
    const before = (await haptics(game)).sequence[1];

    await aimVrHandAt(game, cratePos, 0.45, 1);
    let h = await haptics(game);
    assert.equal(h.sequence[1], before + 1, "entering the accepting crate pulses once");
    assertPulse(h.last[1], TOOL_READY);

    await game.step({ frames: 30 });
    assert.equal((await haptics(game)).sequence[1], before + 1, "lingering does not keep buzzing");

    await aimVrHandAt(game, away, 0.45, 1);
    await game.step({ frames: 3 });
    assert.equal((await haptics(game)).sequence[1], before + 1, "leaving is silent");
    await aimVrHandAt(game, cratePos, 0.45, 1);
    assert.equal((await haptics(game)).sequence[1], before + 2, "re-entry pulses again");
    await game.screenshot("vr-tool-ready-cue.png");

    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    h = await haptics(game);
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(h.sequence[1], before + 3, "the consumed release confirms once");
    assertPulse(h.last[1], TOOL_APPLIED);
    const remaining = (await game.entities.list()).entities.some((e) => e.id === held);
    assert.equal(remaining, false, "the crate consumed the ICE Pick");
    await game.step({ frames: 30 });
    assert.equal((await haptics(game)).sequence[1], before + 3, "an empty hand stops cueing");
  },
);

for (const item of ["Wrench", "Psi Amp"]) {
  test(
    `a held ${item} the crate refuses never cues`,
    { skip: !enabled, timeout: 240_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: ["--vr"] });
      const { cratePos, held } = await setup(game, item);
      const before = (await haptics(game)).sequence[1];
      await aimVrHandAt(game, cratePos, 0.45, 1);
      await game.step({ frames: 10 });
      assert.equal((await haptics(game)).sequence[1], before, `${item} over the crate is silent`);
      await game.input.set("right_hand.squeeze", 0);
      await game.step({ frames: 5 });
      assert.equal((await haptics(game)).sequence[1], before, `releasing ${item} is silent`);
      assert.ok(
        (await game.entities.list()).entities.some((e) => e.id === held),
        `the crate must not consume ${item}`,
      );
    },
  );
}

test(
  "no cue while a squeezing empty hand would catch the item instead",
  { skip: !enabled, timeout: 240_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "hydro1.mis", debugFlags: ["--vr"] });
    const { cratePos, held } = await setup(game, ICE_PICK);
    const before = (await haptics(game)).sequence[1];
    const aim = await aimVrHandAt(game, cratePos, 0.45, 1);
    assert.equal((await haptics(game)).sequence[1], before + 1);
    // Leave, bring the empty left hand in squeezing beside the right, re-enter.
    await aimVrHandAt(game, add(cratePos, [0, 0, 3]), 0.45, 1, 0, { lookAtTarget: false });
    await game.input.set("left_hand.position", add(aim.local, [-0.1, 0, 0]));
    await game.input.set("left_hand.squeeze", 1);
    await game.step({ frames: 3 });
    await aimVrHandAt(game, cratePos, 0.45, 1, 0, { lookAtTarget: false });
    await game.step({ frames: 5 });
    const player = (await game.info()).player;
    assert.equal(player.wielded_entity_id, null, "the left hand stays empty");
    assert.equal((await haptics(game)).sequence[1], before + 1, "a pending handoff is not a ready target");
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.wielded_entity_id, held, "the left hand caught the pick");
  },
);
