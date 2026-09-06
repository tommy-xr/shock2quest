import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";

// The VR wrist readouts and the cyber interface must never show the same
// readout at once (issue #1268). The interface canvas carries the expanded
// BIOFULL/AMMOFULL pair; the wrists carry the watch (health) and the ammo gauge
// in shooter mode. So in EITHER mode there is exactly one copy of each readout.
//
// Negative-first: with `create_wrist_hud_panels` not gated on use mode, opening
// the interface leaves the wrist canvases drawn and the player sees both copies
// - the `player_hands` object count does not drop at all, and the "drops by at
// least the watch" assertions below fail.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_VR_WRIST_PORT ?? 8741);

/** The debug pistol: a magazine, two fire modes, and more than one ammo type. */
const PISTOL = -17;

/**
 * The watch face's own draws: the cropped BIO.PCX plate plus the health row's
 * bar and number (`hud::readouts::build_watch_canvas`). The left wrist alone,
 * so the assertion holds however the right wrist's ammo gauge is populated.
 */
const WATCH_OBJECTS = 3;

/**
 * The least the right wrist adds once a gun is in hand: the cropped AMMOFULL
 * gauge well plus the round count. The ammo icon and type label ride on the
 * weapon's data, so they are not counted here.
 */
const MIN_AMMO_OBJECTS = 2;

/** Objects the VR hand path submitted on the last frame. */
async function handObjectCount(game: GameServer): Promise<number> {
  return (await game.scene.fromSource("player_hands")).length;
}

/** The health number the shared bio layout is showing, off the interface. */
async function interfaceHealth(game: GameServer): Promise<number> {
  const state = await game.ui.state();
  const numbers = state.readout_elements
    .filter((element) => element.kind === "text" && element.text !== undefined)
    .map((element) => Number(element.text));
  assert.ok(numbers.length > 0, "the interface should carry the bio numbers");
  // Health is the first number the bio panel emits (health above psi).
  return numbers[0];
}

test(
  "the wrist readouts go quiet while the cyber interface carries them",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Shooter mode: the wrists are the only copy - the interface is down, so
    // its canvas draws no readout at all.
    const shooter = await game.ui.state();
    assert.deepEqual(
      shooter.readout_elements,
      [],
      "no interface readout while the interface is down",
    );
    const withWrists = await handObjectCount(game);
    assert.ok(
      withWrists >= WATCH_OBJECTS,
      `the watch should be drawn in shooter mode, got ${withWrists} hand objects`,
    );

    // Use mode: the interface canvas takes over, and the wrists go quiet.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const inUse = await game.ui.state();
    assert.equal(inUse.mode, "use");
    assert.ok(
      inUse.readout_elements.some((e) => e.label === "biofull"),
      `expected the interface to carry BIOFULL, got ${inUse.readout_elements.map((e) => e.label)}`,
    );

    const withoutWrists = await handObjectCount(game);
    assert.ok(
      withWrists - withoutWrists >= WATCH_OBJECTS,
      `the wrist canvases must stop drawing in use mode: ${withWrists} -> ${withoutWrists} hand objects`,
    );

    // Leaving use mode hands them back, so the readout is never absent from
    // both places.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    assert.equal((await game.ui.state()).mode, "shooter");
    assert.equal(
      await handObjectCount(game),
      withWrists,
      "the wrist canvases return when the interface goes away",
    );
  },
);

test(
  "a held gun's ammo is on the wrist or the interface, never both",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Empty-handed the right wrist shows nothing at all: only the watch and the
    // two gloves are in the frame.
    const unarmed = await handObjectCount(game);

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

    // Wielding it puts the ammo gauge on the right wrist, beside the watch the
    // left one always wears.
    const armed = await handObjectCount(game);
    assert.ok(
      armed - unarmed >= MIN_AMMO_OBJECTS,
      `wielding must add the wrist ammo gauge: ${unarmed} -> ${armed} hand objects`,
    );
    assert.deepEqual(
      (await game.ui.state()).readout_elements,
      [],
      "the interface draws nothing while the wrists carry the gauge",
    );

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const inUse = await game.ui.state();
    assert.ok(
      inUse.readout_elements.some((e) => e.label === "ammofull"),
      `expected the interface to carry AMMOFULL, got ${inUse.readout_elements.map((e) => e.label)}`,
    );
    assert.ok(
      armed - (await handObjectCount(game)) >= WATCH_OBJECTS + MIN_AMMO_OBJECTS,
      "both wrist canvases must stop drawing while the interface shows them",
    );
  },
);

test(
  "the watch's health follows the player's hit points",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 2,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const player = (await game.info()).player;
    const before = player.hit_points ?? 0;
    const maxHp = player.max_hit_points ?? 0;
    const drawn = await handObjectCount(game);

    assert.ok(player.entity_id !== null, "the scene should have a player");
    await game.entities.sendMessage(player.entity_id, {
      type: "Damage",
      amount: 12,
    });
    await game.step({ frames: 12 });
    const after = (await game.info()).player.hit_points ?? 0;
    assert.ok(
      after < before,
      `the player should have taken damage: ${before} -> ${after}`,
    );

    // The watch is still exactly the same three draws - a health change moves
    // the bar's fill and the number, it does not add or drop elements.
    assert.equal(await handObjectCount(game), drawn);

    // What those draws SAY is the shared bio layout's health row, which the
    // interface exposes verbatim (`hud::readouts` builds both from one
    // `emit_health`, asserted element-for-element in its unit tests). So the
    // number the interface reports is the number on the wrist.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const percent = await interfaceHealth(game);
    assert.equal(percent, Math.round((after / maxHp) * 100));
  },
);
