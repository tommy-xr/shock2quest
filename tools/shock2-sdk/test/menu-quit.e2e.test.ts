import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { AIM_AT_PANEL, menuEntry, panelPoint } from "./helpers/frontend-menu.js";

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

/** Quit is the last of the six main-menu entries. */
const QUIT_ENTRY = menuEntry(5);

test(
  "clicking Quit in the flat menu requests a quit without killing the runtime",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
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

test(
  "the VR hand ray can quit from the menu too",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "main_menu",
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
