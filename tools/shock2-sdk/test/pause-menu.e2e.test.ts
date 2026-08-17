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

// --- #1018 polish: one set of hands, and a dimmed world behind the panel -----
//
// Both claims are made about the frame the renderer was actually handed:
// `/v1/scene` reports every submitted object with the render path that produced
// it, so "the scene's own hands are not drawn" and "a dim layer is drawn" are
// assertions rather than screenshot impressions. Negative-first, against
// `origin/main` at the time of writing: the paused frame carried 8 in-game hand
// objects on top of the menu's own pointer hands, and no dim layer at all.

interface SceneObjectSummary {
  source: string | null;
  transparency: number | null;
  clear_depth: boolean;
  position: [number, number, number];
}

async function sceneObjects(game: GameServer): Promise<SceneObjectSummary[]> {
  const response = await fetch(`${game.baseUrl}/v1/scene`);
  assert.ok(response.ok, `/v1/scene failed: ${response.status}`);
  const body = (await response.json()) as { objects: SceneObjectSummary[] };
  return body.objects;
}

const withSource = (objects: SceneObjectSummary[], source: string) =>
  objects.filter((o) => o.source === source);

test(
  "pausing in VR swaps the scene's hands for the menu's own, and dims the world",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 4,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const running = await sceneObjects(game);
    assert.ok(
      withSource(running, "player_hands").length > 0,
      "the VR scene should draw the player's hands while it is running",
    );
    assert.equal(
      withSource(running, "pause_dim").length,
      0,
      "nothing may dim the world while the player is playing",
    );

    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 30 });
    assert.equal((await game.info()).paused, true);

    const paused = await sceneObjects(game);
    assert.equal(
      withSource(paused, "player_hands").length,
      0,
      "while paused the menu's pointer hands are the only hands (issue #1018)",
    );

    const dim = withSource(paused, "pause_dim");
    assert.equal(dim.length, 1, "exactly one dimming layer, behind the panel");
    assert.ok(
      dim[0].transparency !== null && dim[0].transparency > 0.02 && dim[0].transparency < 0.98,
      `the dim must actually be translucent, got ${dim[0].transparency}`,
    );
    // The panel renders over the world unconditionally (#1017's clear-depth
    // overlay group, explicitly kept). The dim now opens that group, so it has
    // to carry the clear - otherwise a wall in the player's face would swallow
    // the dim and leave the menu floating over a bright world.
    assert.equal(dim[0].clear_depth, true, "the dim opens the overlay group");

    // Resuming puts the world back exactly as it was.
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 30 });
    const resumed = await sceneObjects(game);
    assert.ok(
      withSource(resumed, "player_hands").length > 0,
      "the hands come back on resume",
    );
    assert.equal(
      withSource(resumed, "pause_dim").length,
      0,
      "and the dim goes away with the menu",
    );
  },
);

test(
  "flat presentation is untouched: no dim layer, no hand suppression to do",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 5,
    });
    await game.step({ frames: 30 });
    await game.input.trigger("TogglePauseMenu");
    await game.step({ frames: 30 });
    assert.equal((await game.info()).paused, true);

    // `SIM.PCX` is an opaque full-screen backdrop when it is drawn in screen
    // space, so the flat pause screen already hides the world; adding a world
    // dim there would be a second, differently-tuned answer to a solved
    // problem - and a divergence between the two presentations.
    const paused = await sceneObjects(game);
    assert.equal(
      withSource(paused, "pause_dim").length,
      0,
      "the comfort dim is a VR treatment only",
    );
  },
);
