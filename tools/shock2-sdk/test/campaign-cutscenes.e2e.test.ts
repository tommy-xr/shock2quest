import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { isCutsceneScene, stepPastCutscenes } from "./helpers/cutscenes.js";
import { menuEntry } from "./helpers/frontend-menu.js";

// The authored campaign moments play their movie before the level they lead to.
// Negative-first: before the wiring each of these transitions swapped straight
// to its destination, so every "the cutscene owns the screen" assertion below
// failed with the destination level's name.
//
// Movie names are not present in the mission or gamesys data - the original
// picked them in its engine/game-script code - so this test is also what pins
// the names the code chose.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** New Game is the first of the six main-menu entries. */
const NEW_GAME_ENTRY = menuEntry(0);

// The earth.mis Marine career-door tripwire volume: teleporting onto it fires
// the same ENTER trigger as walking in, running the real TrapNewTripwire ->
// ChooseService chain. Mission-file positions are stable across runs; runtime
// entity ids are not. (See station-flow.e2e.test.ts for the full chain.)
const MARINE_CAREER_DOOR: Vec3 = [-2.4990878, 24.0, 68.16256];

test(
  "New Game plays the intro movie before the first level loads",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({ mission: "main_menu" });
    await game.step({ frames: 10 });

    // Clicks are rising-edge, so the press has to start on a frame where the
    // previous one was unpressed.
    await game.input.set("pointer.position", NEW_GAME_ENTRY);
    await game.step({ frames: 5 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });

    // The intro runs for minutes, so this stops at "the movie is what is on
    // screen, and earth.mis is not loaded yet" rather than playing it out.
    assert.equal(
      (await game.info()).mission,
      "cs1.avi",
      "New Game should show the intro movie before booting the first level",
    );
  },
);

test(
  "enlisting plays the ride to the recruit station and carries the career through it",
  { skip: !e2eEnabled && "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "earth.mis" });
    await game.step({ frames: 5 });

    await game.player.teleport({
      x: MARINE_CAREER_DOOR[0],
      y: MARINE_CAREER_DOOR[1],
      z: MARINE_CAREER_DOOR[2],
    });
    await game.step({ frames: 15 });

    const duringCutscene = await game.info();
    assert.equal(
      duringCutscene.mission,
      "starport.avi",
      "enlisting should play the ride to the recruit station before loading it",
    );
    assert.ok(isCutsceneScene(duringCutscene.mission));

    assert.deepEqual(
      await stepPastCutscenes(game),
      ["starport.avi"],
      "exactly one movie should stand between the career door and the station",
    );

    const arrival = await game.info();
    assert.equal(
      arrival.mission.toLowerCase(),
      "station.mis",
      "the finished movie should hand off to the recruit station",
    );

    // The movie replaces the scene that queued the transition, so its own empty
    // world is what the station would have been handed without the state the
    // cutscene carries: no career bit and no career loadout.
    assert.equal(
      await game.quests.get("career_marine"),
      "complete",
      "the career chosen before the movie should survive it",
    );
    assert.equal(
      arrival.player.max_hit_points,
      45,
      "the Marine loadout should still be applied on the other side of the movie",
    );
  },
);
