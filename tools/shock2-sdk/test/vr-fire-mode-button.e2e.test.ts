import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { describeSounds } from "./helpers/audio.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The gun hand's UPPER face button (right B / left Y) toggles that gun's fire
// mode - the same switch the flat `F` key makes, resolved against the hand that
// pressed it so a dual-wielding player switches the gun they meant.
//
// Negative-first: on the parent a gun hand's upper button resolves to nothing
// (`vr-contextual-buttons` asserts it reaches no panel), so the mode never
// leaves NORM and no `bset` sting plays.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8686);

/** The debug pistol (NORM / BURST). */
const PISTOL = -17;
/** An energy weapon with a second mode of its own (NORM / OVER). */
const LASER_PISTOL = -22;

/** The wielded gun's fire setting and its header, from /v1/info. */
async function fireMode(
  game: GameServer,
): Promise<[number | null, string | null]> {
  const player = (await game.info()).player;
  return [player.wielded_gun_setting, player.wielded_gun_setting_header];
}

async function press(game: GameServer, action: string): Promise<void> {
  await game.input.trigger(action);
  await game.step({ frames: 5 });
}

/** Spawn `template` and take it in the VR RIGHT hand. */
async function grabWeapon(game: GameServer, template: number): Promise<number> {
  // Diffed against the entity list: `debug_weapons` already stocks one of every
  // gun, so a plain template search could hand back the scenery one.
  const weapon = await cycleToWeapon(game, (e) => e.template_id === template);
  await aimVrHandAt(game, weapon.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    weapon.id,
    "the VR right hand must hold the weapon",
  );
  return weapon.id;
}

test(
  "the gun hand's upper button toggles that gun's fire mode",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const pistol = await grabWeapon(game, PISTOL);

    assert.deepEqual(
      await fireMode(game),
      [0, "NORM"],
      "the pistol starts on its first mode",
    );

    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await press(game, "RightHandUpperButton");
    assert.deepEqual(
      await fireMode(game),
      [1, "BURST"],
      "right B switches the gun in that hand to its second mode",
    );

    // The same `bset` sting the flat switch plays.
    const played = (await game.audio.recent()).sounds.filter(
      (s) => s.sequence > before,
    );
    assert.ok(
      played.some((s) => s.sample.toLowerCase().startsWith("bset")),
      `the switch should play the bset sting, got ${describeSounds(played)}`,
    );

    await press(game, "RightHandUpperButton");
    assert.deepEqual(
      await fireMode(game),
      [0, "NORM"],
      "and back - there are only two modes",
    );

    // The button belongs to the hand that PRESSED it: the free left hand's
    // upper button is the log reader's, and leaves the right gun's mode alone.
    await press(game, "RightHandUpperButton");
    assert.deepEqual(await fireMode(game), [1, "BURST"]);
    await press(game, "LeftHandUpperButton");
    assert.deepEqual(
      await fireMode(game),
      [1, "BURST"],
      "the free hand's button must not switch the other hand's gun",
    );
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol,
      "the gun stays in the hand throughout",
    );
  },
);

test(
  "an energy weapon's own second mode switches the same way",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await grabWeapon(game, LASER_PISTOL);

    assert.deepEqual(await fireMode(game), [0, "NORM"]);
    await press(game, "RightHandUpperButton");
    assert.deepEqual(
      await fireMode(game),
      [1, "OVER"],
      "the laser pistol switches to its overcharge",
    );
    await press(game, "RightHandUpperButton");
    assert.deepEqual(await fireMode(game), [0, "NORM"]);
  },
);
