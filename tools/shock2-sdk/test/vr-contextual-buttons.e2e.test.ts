import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The Touch face buttons: the LOWER one (left X / right A) is jump on both
// hands whatever they hold, and the UPPER one (left Y / right B) is contextual
// - the log reader while that hand is free, the weapon's own control while it
// holds one. The cyber interface outranks both, so the buttons can always shut
// it again.
//
// The interface itself is reached from the Menu button now
// (`vr-buttons-v2.e2e.test.ts`), which is what these tests use to open it.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8656);

/** The debug pistol `DebugCycleWeapon` spawns first. */
const PISTOL = -17;

async function uiMode(game: GameServer): Promise<string> {
  return (await game.ui.state()).mode;
}

async function press(game: GameServer, action: string): Promise<void> {
  await game.input.trigger(action);
  await game.step({ frames: 5 });
}

/** Spawn the debug pistol and take it in the VR RIGHT hand. */
async function grabPistol(game: GameServer): Promise<number> {
  // Diffed against the entity list: `debug_weapons` already stocks a pistol,
  // so a plain template search could hand back the scenery one.
  const pistol = await cycleToWeapon(game, (e) => e.template_id === PISTOL);

  await aimVrHandAt(game, pistol.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    pistol.id,
    "the VR right hand must hold the pistol",
  );
  return pistol.id;
}

test(
  "a free hand's upper button reaches the log reader, on either hand",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    assert.equal(await uiMode(game), "shooter");

    // No lower button opens the interface any more: they jump.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must not open the cyber interface",
    );
    await press(game, "LeftHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "and neither must left X",
    );

    // The Menu button is the way in and out.
    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "use");
    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "shooter");
  },
);

test(
  "a gun takes its own hand's upper button, and only its own",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await grabPistol(game);

    // The gun hand's upper button handles the weapon rather than reaching a
    // player-owned panel.
    const header = (await game.info()).player.wielded_gun_setting_header;
    await press(game, "RightHandUpperButton");
    assert.notEqual(
      (await game.info()).player.wielded_gun_setting_header,
      header,
      "right B must switch the fire mode of the gun in that hand",
    );

    // The free hand keeps its own: per-hand, not per-player.
    const switched = (await game.info()).player.wielded_gun_setting_header;
    await press(game, "LeftHandUpperButton");
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      switched,
      "the free left hand must not reach the right hand's gun",
    );
  },
);

test(
  "the open interface takes both buttons back from the hand holding a gun",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 2,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    const pistol = await grabPistol(game);

    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "use", "the Menu button opens it");
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol,
      "the gun stays held while the interface is up (it is safed, not taken)",
    );

    // Mode first: the gun hand's own lower button closes the interface rather
    // than jumping - otherwise the buttons would mean one thing inside a menu
    // and another outside it, on a canvas the player is pointing at.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must close the interface even with a gun in that hand",
    );

    // ...and once it is closed, that button is jump again, not a re-open.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "with the interface down the lower button jumps rather than re-opening it",
    );
  },
);

test(
  "arming the free-camera chord takes the right hand's buttons out of play",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 3,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "use");

    // The chord IS right A+B, and a chord is pressed one button at a time - so
    // while its developer option is on, A must not close the interface on the
    // way to A+B.
    await game.devParams.set("free_camera", 1);
    await game.step({ frames: 2 });
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "use",
      "right A must be inert while the free-camera chord is armed",
    );
    await press(game, "RightHandUpperButton");
    assert.equal(await uiMode(game), "use", "and so must right B");

    // The left hand keeps its buttons - the chord is right-handed.
    await press(game, "LeftHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "the left hand is unaffected by the chord",
    );
    await press(game, "MenuButton");

    // Disarmed again, the right hand gets them back.
    await game.devParams.set("free_camera", 0);
    await game.step({ frames: 2 });
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A closes the interface again once the chord is disarmed",
    );
  },
);
