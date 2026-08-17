import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

// End-to-end test for the main menu's Quit entry: clicking it must reach
// `Game::should_quit`, which is what every runtime shuts down on (the desktop
// closes its window, the Quest asks OpenXR to exit its session).
//
// The debug runtime deliberately does NOT quit - an automation session must not
// kill itself - so it reports the request through `/v1/info`'s `quit_requested`
// instead, which is what makes the whole path assertable headlessly.
//
// Negative-first: without `quit_requested` on the snapshot the field is
// `undefined` and both assertions below fail.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// MAINR.BIN stacks six 179x60 buttons at x=400 on a 76px pitch, on the 640x480
// UI canvas; Quit is the last one (index 5), centered at y=430.
const CANVAS_W = 640;
const CANVAS_H = 480;
const QUIT_ENTRY: [number, number] = [(400 + 179 / 2) / CANVAS_W, (20 + 5 * 76 + 30) / CANVAS_H];

test(
  "clicking Quit in the flat menu requests a quit without killing the runtime",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu", port: 8126 });
    await game.step({ frames: 10 });

    assert.equal(
      (await game.info()).quit_requested,
      false,
      "nothing has asked to quit yet",
    );

    // Clicks are rising-edge, so the press has to start on a frame where the
    // previous one was unpressed.
    await game.input.set("pointer.position", QUIT_ENTRY);
    await game.step({ frames: 5 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.info()).quit_requested,
      true,
      "clicking Quit should ask the game to quit",
    );
    // Still serving: the debug runtime is quit-immune on purpose.
    assert.equal((await game.info()).mission, "main_menu");
  },
);

// The VR menu is the same canvas on a world panel, driven by a controller ray -
// so the Quest's Quit has to travel the same path (AGENTS.md: "a canvas must
// render the same way in flatscreen and in VR").
//
// The runtime's VR head yaw is +90 degrees (the camera looks along -X), so the
// panel hangs at x=-2 facing the head: canvas +x maps to world -z and +y to -y.
const PANEL_DISTANCE = 2;
const PANEL_SIZE = { x: 2, y: 1.5 };
/** 90-degree yaw: rotates a hand's -Z ray onto the panel's -X. */
const AIM_AT_PANEL: [number, number, number, number] = [0, 0.7071068, 0, 0.7071068];

/** Pawn-local position of a canvas point on the VR panel. */
const panelPoint = ([u, v]: [number, number]): [number, number, number] => [
  -PANEL_DISTANCE,
  PLAYER_EYE_HEIGHT_WORLD + (0.5 - v) * PANEL_SIZE.y,
  (0.5 - u) * PANEL_SIZE.x,
];

test(
  "the VR hand ray can quit from the menu too",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
      port: 8127,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 10 });

    assert.equal((await game.info()).quit_requested, false);

    // Origin at the button's own world position, aimed straight down -X onto it.
    const [, y, z] = panelPoint(QUIT_ENTRY);
    await game.input.set("right_hand.rotation", AIM_AT_PANEL);
    await game.input.set("right_hand.position", [0, y, z]);
    await game.step({ frames: 10 });

    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 5 });

    assert.equal(
      (await game.info()).quit_requested,
      true,
      "the trigger over Quit should ask the game to quit",
    );
    assert.equal((await game.info()).mission, "main_menu");
  },
);
