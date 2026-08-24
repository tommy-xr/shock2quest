import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import type { Vec3 } from "../src/types.js";

// The loading screen used to draw ONLY in screen space, so a level transition
// in the headset showed nothing at all (#1002): the compositor kept presenting
// the last menu frame for the whole load. It now maps the same canvas onto a
// world-space frontend panel, like every other frontend screen.
//
// `/v1/scene` reports what the renderer was handed last frame - both the world
// objects from `render` and the screen-space quads from `render_per_eye` - so
// the two presentations are told apart by WHERE the canvas landed, not by how
// much of it drew: a screen-space quad carries no world transform and reports
// the origin, while the panel's quads sit at the placed panel, one frontend
// panel distance along the head's gaze.
//
// Negative-first: against the previous screen-space-only LoadingScene the VR
// test fails - its canvas drew at the origin like the flat one, because
// `render` returned nothing in either presentation.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Where the renderer was handed each object on the last frame. */
async function drawnAt(game: GameServer): Promise<Vec3[]> {
  const { objects } = await game.scene.objects();
  return objects.map((object) => object.position);
}

/**
 * How far an object was drawn from the eye.
 *
 * The frontend scenes run in an empty world, so the pawn - and therefore the
 * eye - sits at the origin at eye height. Measuring against that rather than
 * against a copy of `FRONTEND_PANEL_DISTANCE` keeps the assertions about the
 * property that actually separates the presentations (is the canvas out in the
 * world, at a fixed radius, or pasted at the origin?) instead of pinning a
 * number that lives in Rust.
 */
const distanceFromEye = ([x, y, z]: Vec3): number =>
  Math.hypot(x, y - PLAYER_EYE_HEIGHT_WORLD, z);

const distanceFromOrigin = ([x, y, z]: Vec3): number => Math.hypot(x, y, z);

/**
 * Reload the level in place and stop while the loading screen is up.
 *
 * The deferred transition parses on a worker thread and holds the loading
 * screen for a minimum number of frames, so a few steps land squarely inside
 * it.
 */
async function enterLoadingScreen(game: GameServer): Promise<void> {
  await game.step({ frames: 30 });
  await game.input.trigger("DebugReloadLevel");
  await game.step({ frames: 3 });
  assert.equal(
    (await game.info()).mission,
    "loading",
    "the deferred transition should be showing the loading screen",
  );
}

test(
  "the VR loading screen draws its canvas on a world panel",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      experimental: ["loading_screen"],
      debugFlags: ["--vr"],
    });

    await enterLoadingScreen(game);

    // Backdrop + disc + progress bar, each a quad on the panel.
    const positions = await drawnAt(game);
    assert.ok(
      positions.length >= 3,
      `the loading screen should draw its whole canvas, drew ${positions.length} objects`,
    );

    // Every one of them is out in the world on one flat panel, not pasted at
    // the origin: each sits well clear of the eye, all at the same radius (the
    // per-layer z-step spreads them by a millimetre, hence the tolerance), and
    // all at eye height because the placement is gravity-aligned.
    const radii = positions.map(distanceFromEye);
    for (const [index, position] of positions.entries()) {
      assert.ok(
        radii[index] > 1,
        `a VR loading-screen element should be out in the world, drew at ${JSON.stringify(position)}`,
      );
      assert.ok(
        Math.abs(radii[index] - radii[0]) < 0.05,
        `the panel is flat, so every element shares a radius; drew at ${JSON.stringify(position)}`,
      );
      assert.ok(
        Math.abs(position[1] - PLAYER_EYE_HEIGHT_WORLD) < 0.05,
        `the panel hangs at eye level, drew at ${JSON.stringify(position)}`,
      );
    }
  },
);

test(
  "the flat loading screen stays in screen space",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      experimental: ["loading_screen"],
      debugFlags: [],
    });

    await enterLoadingScreen(game);

    // Flat draws the same canvas as a screen-space overlay and nothing in the
    // world - a world panel here would sit in front of a screen-space copy.
    // This is the guard that the VR work left the flat presentation alone.
    const positions = await drawnAt(game);
    assert.ok(
      positions.length >= 3,
      `the loading screen should draw its whole canvas, drew ${positions.length} objects`,
    );
    for (const position of positions) {
      assert.equal(
        distanceFromOrigin(position),
        0,
        `a flat loading-screen element is screen-space, drew at ${JSON.stringify(position)}`,
      );
    }
  },
);
