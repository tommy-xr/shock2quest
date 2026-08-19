import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { SceneObjectSummary } from "../src/types.js";

// `arm_offset` (shock2vr::dev_params::ARM_HEIGHT_OFFSET) is the VR comfort knob
// for "my in-game arms do not sit where my physical arms do". It is applied in
// `Game::update`, which the Quest reaches through `App::update` and the debug
// runtime reaches directly, so what this measures here is the same code path
// the headset takes. (What it cannot measure is the tracked pose upstream of
// that; only a worn check covers that half.)
//
// The claim under test is live-apply on the *rendered* hands: POST a value,
// step, and the objects the renderer was handed on the `player_hands` path have
// risen by exactly the POSTed metres, converted into world units. The registry
// protocol itself (metadata, clamping, reset, unknown keys) belongs to
// `dev-params.e2e.test.ts` and is deliberately not re-proven per knob.
//
// Negative-first: with the offset applied one level up in `App::update` - which
// is where it started, and which the Quest does read - the debug runtime never
// sees it and the delta assertion fails at 0.000.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** shock2vr::METERS_PER_WORLD_UNIT (0.3048 * dark::SCALE_FACTOR). */
const METERS_PER_WORLD_UNIT = 0.3048 * 2.5;

const HANDS = "player_hands";

/**
 * Mean height of everything the hands render path drew. The path emits several
 * objects per hand (glove, sleeve, forearm HUD panel and its overlay layers);
 * the offset is a rigid translation of all of them, so their mean moves by
 * exactly the offset while being far less noisy than any single pick.
 */
function handHeight(objects: SceneObjectSummary[]): number {
  assert.ok(objects.length > 0, "expected the VR hands to be drawn");
  return objects.reduce((sum, o) => sum + o.position[1], 0) / objects.length;
}

test(
  "the arm-height dev param moves the rendered VR hands, live",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8231),
      debugFlags: ["--vr"],
    });
    // Long enough for the player to settle on the floor, so the only thing
    // that moves the hands afterwards is the knob.
    await game.step({ frames: 90 });

    const { params } = await game.devParams.list();
    const armOffset = params.find((p) => p.key === "arm_offset");
    assert.ok(armOffset, "arm_offset must be registered");
    assert.equal(armOffset.default, 0, "the knob must ship inert");

    const before = handHeight(await game.scene.fromSource(HANDS));
    const shotBefore = await game.screenshot("arm-height-0.png");

    // Live-apply: the registry is read every frame, so two frames is plenty.
    const raiseMeters = 0.3;
    const applied = await game.devParams.set("arm_offset", raiseMeters);
    assert.ok(Math.abs(applied.value - raiseMeters) < 1e-4);
    await game.step({ frames: 2 });

    const after = handHeight(await game.scene.fromSource(HANDS));
    const expected = raiseMeters / METERS_PER_WORLD_UNIT;
    assert.ok(
      Math.abs(after - before - expected) < 0.02,
      `hands should have risen by ${expected.toFixed(3)} world units, ` +
        `got ${(after - before).toFixed(3)} (${before.toFixed(3)} -> ${after.toFixed(3)})`,
    );
    const shotAfter = await game.screenshot("arm-height-0.3.png");
    console.log(
      `hand height ${before.toFixed(3)} -> ${after.toFixed(3)}; ` +
        `screenshots: ${shotBefore.full_path} ${shotAfter.full_path}`,
    );

    // The knob lowers as well as raises - a sign error would still have passed
    // the raise assertion if it had been read from the wrong end of the range.
    await game.devParams.set("arm_offset", -0.3);
    await game.step({ frames: 2 });
    const lowered = handHeight(await game.scene.fromSource(HANDS));
    assert.ok(
      Math.abs(before - lowered - expected) < 0.02,
      `a negative offset must lower the hands by ${expected.toFixed(3)}, ` +
        `got ${(before - lowered).toFixed(3)}`,
    );
  },
);
