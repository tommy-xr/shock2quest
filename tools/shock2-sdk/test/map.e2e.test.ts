import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the flat-mode automap panel (projects/flat-ui-panels.md
// §5, flat UI 6c): visited-region tracking (persisted in QuestInfo), the
// room->MapLoc reveal wiring, the MapRef world->page transform driving the
// player marker, and the wide unbound (sticky) panel opened by ToggleMap.
//
// Negative-first: on the base branch, POST /v1/input/action {"ToggleMap"}
// returns "Unknown action", /v1/ui never shows a map panel, and /v1/info has
// no explored_map_locations field - every assertion below fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "automap: ToggleMap opens the wide panel, rooms reveal, marker tracks, persists",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8167),
    });
    await game.step({ frames: 10 });

    // The spawn room is mapped: entering it at load reveals its location.
    const atStart = (await game.info()).player.explored_map_locations;
    assert.ok(
      atStart.length >= 1,
      `spawning inside a mapped room should reveal its location (got ${atStart})`,
    );
    assert.ok(!(await game.ui.state()).active_panel, "no panel before ToggleMap");

    // --- ToggleMap opens the wide map panel ---
    await game.input.trigger("ToggleMap");
    await game.step({ frames: 5 });
    const opened = await game.ui.state();
    assert.ok(opened.active_panel, "ToggleMap should open the map panel");
    const textures = () =>
      opened.active_panel!.elements
        .filter((e) => e.kind === "image")
        .map((e) => (e.texture ?? "").toLowerCase());
    const backdrop = opened.active_panel.elements.find(
      (e) => (e.texture ?? "").toLowerCase() === "mapback.pcx",
    );
    assert.ok(backdrop, `panel should draw MAPBACK (got ${textures()})`);
    // The wide panel: 636x296 anchored at the left-MFD slot, filling the
    // canvas width (the original's {2,2}-{638,302} both-slot rect).
    assert.deepEqual(backdrop.rect, [2, 124, 636, 296]);
    assert.ok(
      textures().some((t) => t.endsWith("page001.pcx")),
      "panel should draw the level's PAGE001 art",
    );
    assert.ok(
      textures().some((t) => /p001r\d{3}\.pcx$/.test(t)),
      "the spawn room's revealed decal should be drawn",
    );

    // --- Player marker present, and it tracks movement ---
    const marker = () =>
      game.ui.state().then(
        (s) =>
          s.active_panel?.elements.find(
            (e) => (e.texture ?? "").toLowerCase() === "plrpip.pcx",
          ) ?? null,
      );
    const markerBefore = await marker();
    assert.ok(markerBefore, "panel should draw the player marker (plrpip)");

    // Walk forward while the map is open: the unbound panel must NOT
    // walk-away close (it has no bound world object), and the marker moves.
    await game.input.set("right_hand.thumbstick", [0.0, 1.0]);
    await game.step({ frames: 90 });
    await game.input.set("right_hand.thumbstick", [0.0, 0.0]);
    const markerAfter = await marker();
    assert.ok(markerAfter, "map panel must survive walking (no distance close)");
    const moved = Math.hypot(
      markerAfter.rect[0] - markerBefore.rect[0],
      markerAfter.rect[1] - markerBefore.rect[1],
    );
    assert.ok(
      moved > 1.0,
      `the player marker should move with the player (moved ${moved.toFixed(2)}px)`,
    );

    // Marker accuracy: recompute the world->page mapping from the two known
    // medsci1 MapRef scale markers and check the drawn marker lands within a
    // few pixels of the player's true position.
    const pos = await game.player.position();
    // Solved from mission data (MapRef frame:-1 markers 1032/1034); medsci1's
    // page is axis-swapped (world z -> page x, world x -> page y).
    const sx = (232 - 536) / (-76.398605 - 32.874435);
    const bx = 536 - sx * 32.874435;
    const sy = (10 - 239) / (44.685417 - -40.31117);
    const by = 239 - sy * -40.31117;
    const expectX = 2 + 11 + (sx * pos.z + bx) - 8; // canvas = anchor + page offset + page px - half marker
    const expectY = 124 + 18 + (sy * pos.x + by) - 8;
    assert.ok(
      Math.abs(markerAfter.rect[0] - expectX) < 4 &&
        Math.abs(markerAfter.rect[1] - expectY) < 4,
      `marker should track the true position (got [${markerAfter.rect[0].toFixed(1)}, ` +
        `${markerAfter.rect[1].toFixed(1)}], expected [${expectX.toFixed(1)}, ${expectY.toFixed(1)}])`,
    );

    // --- Entering another mapped room grows the explored set (and its decal
    // appears). The elevator lobby is location 4 in medsci1. ---
    await teleportVerified(game, { x: 2.5, y: 0.5, z: -40.4 });
    await game.step({ frames: 20 });
    const grown = (await game.info()).player.explored_map_locations;
    assert.ok(
      grown.length > atStart.length,
      `entering the elevator lobby should reveal its location (${atStart} -> ${grown})`,
    );
    const panelNow = await game.ui.state();
    const decals = panelNow.active_panel!.elements.filter((e) =>
      /p001r\d{3}\.pcx$/i.test(e.texture ?? ""),
    );
    assert.equal(
      decals.length,
      grown.length,
      "one revealed decal per explored location",
    );
    await game.screenshot("map-panel-revealed.png");

    // --- Second ToggleMap closes the panel ---
    await game.input.trigger("ToggleMap");
    await game.step({ frames: 3 });
    assert.ok(
      !(await game.ui.state()).active_panel,
      "a second ToggleMap should close the map",
    );

    // --- The explored set survives save/load ---
    await game.save("map-e2e");
    await game.step({ frames: 2 });
    await game.load("map-e2e");
    await game.step({ frames: 5 });
    assert.deepEqual(
      (await game.info()).player.explored_map_locations,
      grown,
      "explored locations should survive save/load",
    );
  },
);
