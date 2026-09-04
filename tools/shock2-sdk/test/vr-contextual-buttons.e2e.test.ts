import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/index.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The Touch face buttons are per-hand and contextual: lower (left X / right A)
// and upper (left Y / right B) mean the player-owned panels while that hand is
// free, and belong to the weapon while it holds one. The interface outranks
// both, so the press that opened it always closes it.
//
// Negative-first: on the parent these actions do not exist at all
// (`/v1/input/action` rejects the name), and right A/B carried nothing.
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
  "a free right hand's lower button opens and closes the cyber interface",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    assert.equal(await uiMode(game), "shooter");

    // Right A, mirroring left X - the new half of the symmetry.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "use",
      "right A with an empty hand must open the cyber interface",
    );

    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must close what it opened",
    );

    // The left hand's own buttons are unchanged.
    await press(game, "LeftHandLowerButton");
    assert.equal(await uiMode(game), "use", "left X must still open it");
    await press(game, "LeftHandLowerButton");
    assert.equal(await uiMode(game), "shooter");
  },
);

test(
  "a gun takes its own hand's buttons, and only its own",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await grabPistol(game);

    // The gun hand's buttons are the gun's now - and nothing is bound to them
    // yet, so they do nothing at all rather than reaching the panels.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must not open the interface while that hand holds a gun",
    );
    await press(game, "RightHandUpperButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right B must not open the reader while that hand holds a gun",
    );

    // The free hand still owns the panels: per-hand, not per-player.
    await press(game, "LeftHandLowerButton");
    assert.equal(
      await uiMode(game),
      "use",
      "the free left hand must still reach the interface",
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

    await press(game, "LeftHandLowerButton");
    assert.equal(await uiMode(game), "use", "the free hand opens it");
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      pistol,
      "the gun stays held while the interface is up (it is safed, not taken)",
    );

    // Mode first: the gun hand's own button closes the interface rather than
    // being swallowed by the weapon it holds - otherwise picking a gun up
    // inside the interface could strand the player in it.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must close the interface even with a gun in that hand",
    );

    // ...and once it is closed, that button belongs to the gun again.
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "with the interface down the gun hand's button is inert again",
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

    // The chord IS right A+B, and a chord is pressed one button at a time -
    // so while its developer option is on, A must not open the interface on
    // the way to A+B.
    await game.devParams.set("free_camera", 1);
    await game.step({ frames: 2 });
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "shooter",
      "right A must be inert while the free-camera chord is armed",
    );
    await press(game, "RightHandUpperButton");
    assert.equal(await uiMode(game), "shooter", "and so must right B");

    // The left hand keeps its buttons - the chord is right-handed.
    await press(game, "LeftHandLowerButton");
    assert.equal(
      await uiMode(game),
      "use",
      "the left hand is unaffected by the chord",
    );
    await press(game, "LeftHandLowerButton");

    // Disarmed again, the right hand gets them back.
    await game.devParams.set("free_camera", 0);
    await game.step({ frames: 2 });
    await press(game, "RightHandLowerButton");
    assert.equal(
      await uiMode(game),
      "use",
      "right A works again once the chord is disarmed",
    );
  },
);
