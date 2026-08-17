import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The in-game pause menu is a `Game`-level overlay, not a scene: it draws over
// whatever gameplay scene is active and skips that scene's update while it is
// up. These tests pin the three things that make it a pause menu rather than a
// screen that happens to be showing:
//
//   1. the simulation really is frozen (and really does resume),
//   2. it works over a debug scene as well as a mission - the whole reason it
//      lives on `Game`,
//   3. the edge guards: it never opens over a frontend screen or a dead
//      player, and a press already held when it opens cannot click an entry.
//
// Negative-first: before the feature, (1) fails because `TogglePauseMenu` is
// not an action at all, and against a version that skips the scene update but
// keeps `Game::render` gated the world stops drawing - which is the VR
// comfort/compositor bug (#1002/#1003) this design exists to avoid.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8210);

const CANVAS_W = 640;
const CANVAS_H = 480;
// SIMR.BIN: five 179x76 buttons at x=400, top 20, on a 92px pitch.
const entry = (index: number): [number, number] => [
  (400 + 179 / 2) / CANVAS_W,
  (20 + index * 92 + 76 / 2) / CANVAS_H,
];
const CONTINUE = entry(0);
const SAVE_GAME = entry(1);
const QUIT_TO_MAIN_MENU = entry(4);

/** Walk forward for `frames` and report where the player ended up. */
async function walk(game: GameServer, frames: number): Promise<[number, number, number]> {
  await game.input.set("right_hand.thumbstick", [0, 1]);
  await game.step({ frames });
  const { position } = await game.info().then((i) => ({ position: i.player.position }));
  return position as [number, number, number];
}

/** Click a canvas point with the flat pointer, as a real rising edge. */
async function click(game: GameServer, [u, v]: [number, number]): Promise<void> {
  await game.input.set("pointer.position", [u, v]);
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 3 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 3 });
}

test(
  "the pause menu freezes the mission and resumes it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 30 });

    assert.ok(
      (await game.input.actions()).includes("TogglePauseMenu"),
      "TogglePauseMenu should be in the action vocabulary",
    );

    // Walking, then paused mid-stride with the stick still held forward.
    const walking = await walk(game, 60);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 60 });

    const paused = await game.info();
    assert.equal(paused.paused, true, "the menu should be up");
    assert.deepEqual(
      paused.player.position,
      walking,
      "the player must not move while paused, stick held or not",
    );

    // Still frozen three seconds later - the sim is stopped, not just slow.
    await game.step({ frames: 180 });
    const stillPaused = await game.info();
    assert.deepEqual(stillPaused.player.position, walking, "the sim must stay frozen");

    // A dimmed stub swallows its click: still paused afterwards.
    await click(game, SAVE_GAME);
    assert.equal(
      (await game.info()).paused,
      true,
      "Save Game is a dimmed stub and must not resume",
    );

    // "Continue" resumes, and the sim advances again.
    await click(game, CONTINUE);
    const resumed = await game.info();
    assert.equal(resumed.paused, false, "Continue should resume");

    await game.step({ frames: 60 });
    const after = await game.info();
    assert.notDeepEqual(
      after.player.position,
      resumed.player.position,
      "the held stick should move the player again once resumed",
    );
  },
);

test(
  "Quit to Main Menu leaves the mission, and the menu cannot be paused",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 1,
    });
    await game.step({ frames: 30 });

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 10 });
    assert.equal((await game.info()).paused, true);

    await click(game, QUIT_TO_MAIN_MENU);
    await game.step({ frames: 10 });

    const menu = await game.info();
    assert.equal(menu.mission, "main_menu", "Quit should land on the main menu");
    assert.equal(menu.paused, false, "the pause overlay closes with the mission");

    // A frontend screen is already system UI: pausing it would stack two menus
    // and hand the pointer to the wrong one.
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 20 });
    assert.equal(
      (await game.info()).paused,
      false,
      "the pause menu must not open over the main menu",
    );
  },
);

test(
  "a debug scene pauses too - the point of owning this above the scene",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_minimal",
      port: basePort + 2,
    });
    await game.step({ frames: 30 });

    const walking = await walk(game, 60);
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 120 });

    const paused = await game.info();
    assert.equal(paused.paused, true, "debug scenes are pausable");
    assert.deepEqual(paused.player.position, walking, "the debug scene must freeze too");

    await click(game, CONTINUE);
    await game.step({ frames: 60 });
    const after = await game.info();
    assert.equal(after.paused, false);
    assert.notDeepEqual(after.player.position, walking, "and resume");
  },
);

test(
  "a dead player owns the moment - the pause menu stays shut",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 3,
    });
    await game.step({ frames: 30 });

    const before = await game.info();
    assert.ok(before.player.entity_id !== null && before.player.hit_points !== null);
    await game.entities.sendMessage(before.player.entity_id, {
      type: "Damage",
      amount: before.player.hit_points + 100,
    });
    await game.step({ frames: 10 });
    assert.notEqual(
      (await game.info()).player.life_state,
      "alive",
      "the player should be dead or dying",
    );

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 20 });
    assert.equal(
      (await game.info()).paused,
      false,
      "the death sequence and game-over screen own this moment",
    );
  },
);
