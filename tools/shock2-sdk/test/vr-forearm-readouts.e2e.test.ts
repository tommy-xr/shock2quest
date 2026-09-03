import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// The VR forearm HUD panels and the cyber interface must never show the same
// readout at once (issue #1268). Since #1267 the interface canvas carries the
// expanded BIOFULL/AMMOFULL pair; the forearms carry them in shooter mode. So
// in EITHER mode there is exactly one copy of each readout in the frame.
//
// Negative-first: on the parent branch `create_arm_hud_panels` is not gated on
// use mode, so opening the interface leaves the forearm panels drawn and the
// player sees both copies - the `player_hands` object count does not drop at
// all, and the "drops by at least the bio panel" assertions below fail.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_VR_FOREARM_PORT ?? 8741);

/** The debug pistol: a magazine, two fire modes, and more than one ammo type. */
const PISTOL = -17;

/**
 * The BIOFULL forearm panel's own draws: the backdrop quad plus the four
 * elements `hud::readouts::emit_bio` overlays on it (two bars, two numbers).
 * The left arm alone, so the assertion holds however the right arm's ammo
 * gauge is populated.
 */
const BIO_FOREARM_OBJECTS = 5;

/** Objects the VR hand path submitted on the last frame. */
async function handObjectCount(game: GameServer): Promise<number> {
  return (await game.scene.fromSource("player_hands")).length;
}

test(
  "the forearm readouts go quiet while the cyber interface carries them",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Shooter mode: the forearms are the only copy - the interface is down, so
    // its canvas draws no readout at all.
    const shooter = await game.ui.state();
    assert.deepEqual(
      shooter.readout_elements,
      [],
      "no interface readout while the interface is down",
    );
    const withForearms = await handObjectCount(game);
    assert.ok(
      withForearms >= BIO_FOREARM_OBJECTS,
      `the forearms should be drawn in shooter mode, got ${withForearms} hand objects`,
    );

    // Use mode: the interface canvas takes over, and the arms go quiet.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const inUse = await game.ui.state();
    assert.equal(inUse.mode, "use");
    assert.ok(
      inUse.readout_elements.some((e) => e.label === "biofull"),
      `expected the interface to carry BIOFULL, got ${inUse.readout_elements.map((e) => e.label)}`,
    );

    const withoutForearms = await handObjectCount(game);
    assert.ok(
      withForearms - withoutForearms >= BIO_FOREARM_OBJECTS,
      `the forearm panels must stop drawing in use mode: ${withForearms} -> ${withoutForearms} hand objects`,
    );

    // Leaving use mode hands them back, so the readout is never absent from
    // both places.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    assert.equal((await game.ui.state()).mode, "shooter");
    assert.equal(
      await handObjectCount(game),
      withForearms,
      "the forearm panels return when the interface goes away",
    );
  },
);

test(
  "a held gun's ammo is on the forearm or the interface, never both",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const pistol = (await game.entities.list()).entities.find(
      (e) => e.template_id === PISTOL,
    );
    assert.ok(pistol, "the scene should stock the debug pistol");
    await aimVrHandAt(game, pistol.position as Vec3, 0.35);
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 8 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol.id,
      "the VR right hand must hold the pistol",
    );

    // Wielding it adds the ammo gauge to the right forearm, on top of the bio
    // panel the left arm always wears.
    const armed = await handObjectCount(game);
    assert.deepEqual(
      (await game.ui.state()).readout_elements,
      [],
      "the interface draws nothing while the arms carry the gauge",
    );

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const inUse = await game.ui.state();
    assert.ok(
      inUse.readout_elements.some((e) => e.label === "ammofull"),
      `expected the interface to carry AMMOFULL, got ${inUse.readout_elements.map((e) => e.label)}`,
    );
    assert.ok(
      armed - (await handObjectCount(game)) >= BIO_FOREARM_OBJECTS,
      "both forearm panels must stop drawing while the interface shows them",
    );
  },
);
