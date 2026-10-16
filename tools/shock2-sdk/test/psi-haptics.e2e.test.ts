import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// The VR psi amp's haptics, on the hand holding it: a charge rumble that
// swells, one detent entering the overload zone, then the cast, overload,
// burnout and fizzle pulses. Cryokinesis (the default) is tier 1 - a 2.0s
// = 120-frame bar with the overload zone from 85%.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

type Pulse = { amplitude: number; duration_ms: number } | null;
// Amplitudes are f32 in the runtime: 0.8 arrives as 0.800000011920929.
const is = (p: Pulse, amplitude: number, duration_ms?: number) =>
  p !== null && Math.abs(p.amplitude - amplitude) < 1e-6 &&
  (duration_ms === undefined || p.duration_ms === duration_ms);

test("psi amp haptics follow the charge and its outcome", { skip: !e2eEnabled, timeout: 600_000 }, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi", debugFlags: ["--vr"] });
  await game.step({ frames: 10 });
  const [amp] = await game.entities.byTemplate(-247);
  await aimVrHandAt(game, amp.position, 0.35);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal((await game.info()).player.right_hand_entity_id, amp.id);
  await game.step({ frames: 30 });

  // `last`, not `pending`: a trigger edge lands in one of the runtime's idle
  // zero-dt frames between requests, whose next frame consumes `pending`.
  const last = async (): Promise<[Pulse, Pulse]> =>
    (await game.info()).player.hand_feedback!.haptics!.last as [Pulse, Pulse];
  // Step one frame at a time, collecting the amp (right) hand's latest pulse;
  // the free left hand must stay silent throughout.
  const frames = async (n: number) => {
    const pulses: Pulse[] = [];
    for (let i = 0; i < n; i++) {
      await game.step({ frames: 1 });
      const [left, right] = await last();
      assert.equal(left, null, "only the amp hand is cued");
      pulses.push(right);
    }
    return pulses;
  };

  // Charge to inside the zone, then release: overload cast.
  await game.input.set("right_hand.trigger", 1);
  const charge = await frames(108);
  const early = charge[20]!.amplitude;
  const late = charge[90]!.amplitude;
  assert.ok(early > 0 && late > early, `rumble swells (${early} -> ${late})`);
  const detents = charge.filter((p) => is(p, 0.8));
  assert.equal(detents.length, 1, "one detent entering the overload zone");
  assert.ok(is(charge.at(-1)!, 0.45), "steady buzz inside the zone");
  await game.input.set("right_hand.trigger", 0);
  assert.ok(is((await frames(1))[0], 1, 120), "overload cast");
  await game.step({ frames: 60 });

  // Quick click: a normal cast.
  await game.input.set("right_hand.trigger", 1);
  await frames(3);
  await game.input.set("right_hand.trigger", 0);
  assert.ok(is((await frames(1))[0], 0.55, 50), "normal cast");
  await game.step({ frames: 60 });

  // Over-hold: burnout.
  await game.input.set("right_hand.trigger", 1);
  const held = await frames(125);
  assert.ok(
    held.some((p) => is(p, 1, 350)),
    "burnout jolt",
  );
  await game.input.set("right_hand.trigger", 0);
  await game.step({ frames: 60 });

  // Changing the power mid-charge fizzles it.
  await game.input.set("right_hand.trigger", 1);
  await frames(10);
  await game.input.trigger("CyclePsiPower");
  const cycled = await frames(3);
  assert.ok(
    cycled.some((p) => is(p, 0.2, 90)),
    `fizzle (${JSON.stringify(cycled)})`,
  );
});
