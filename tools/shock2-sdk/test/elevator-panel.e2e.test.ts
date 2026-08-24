import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the flat-mode elevator MFD (projects/flat-ui-panels.md
// §2, PR A - "flat UI 6a"): frobbing the medsci1 Master Elevator Button opens
// ElevatorGui in the left MFD, whose floor buttons are now semantically
// labeled from MISC.STR (`ElevLevel<n>`), the current deck is lit + inert, and
// floors gate on the `ElevState` quest bit. With no power the original draws
// POWER.PCX instead of any floor controls; partial power reaches decks 1..=3;
// full power reaches all five. Clicking an available floor transitions the
// game to that mission (marker StartLoc 22).
//
// Entity discovery is by NAME at run time (runtime entity ids are NOT stable
// across launches): the "Master Elevator Button" (medsci1 mission id 1041,
// script ElevatorButton). The floor labels/decks are global data (the gamesys
// `Elev` file-var), so the button carries no floor links.
//
// Issue #593 negative-first: current main renders the normal ELEV.PCX panel
// and exposes every non-current floor while ElevState is unset. It also gates
// partial power backwards, exposing decks 4..=5 instead of decks 1..=3.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The five floor labels ElevatorGui reads from MISC.STR (deck 1..5). Med / Sci
// (deck 2) is the current floor in medsci1 - lit + inert, never a button.
//
// Matched on deck name + number rather than the exact string, because the label
// text is data and mods rewrite it: `ElevLevel1` is "Engineering (1)" in the
// original MISC.STR and "1: Engineering" in SCP's. What this test is actually
// about is that the floor is labeled from MISC.STR at all, and which deck it
// points at - not one release's phrasing.
type Floor = { name: RegExp; deck: number; describe: string };
const ENGINEERING: Floor = {
  name: /engineering/i,
  deck: 1,
  describe: "Engineering (deck 1)",
};
const OPERATIONS: Floor = {
  name: /operations/i,
  deck: 4,
  describe: "Operations (deck 4)",
};
const HYDROPONICS: Floor = {
  name: /hydroponics/i,
  deck: 3,
  describe: "Hydroponics (deck 3)",
};
const MED_SCI: Floor = {
  name: /med\s*\/?\s*sci/i,
  deck: 2,
  describe: "Med / Sci (deck 2)",
};

const labelsFloor = (label: string | null | undefined, floor: Floor): boolean =>
  !!label && floor.name.test(label) && label.includes(String(floor.deck));

const floorButton = (state: UiState, floor: Floor): UiElement | undefined =>
  state.active_panel?.elements.find(
    (e) => e.kind === "button" && labelsFloor(e.label, floor),
  );

test(
  "flat elevator MFD: power states, labeled floors, current-floor inert, click transitions",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    const uiState = () => game.ui.state();

    // --- Discover the Master Elevator Button by name ---
    const buttons = (
      await game.entities.list({ filter: "Master Elevator Button", limit: 20 })
    ).entities;
    assert.ok(
      buttons.length > 0,
      "medsci1 should contain a 'Master Elevator Button'",
    );
    const buttonId = buttons[0].id;
    const button = await game.entities.detail(buttonId);

    // --- Get the player next to the button (within the 4u auto-close radius) ---
    const [bx, by, bz] = button.position;
    await teleportVerified(game, { x: bx + 1.0, y: by + 0.5, z: bz + 1.0 });
    await game.step({ frames: 10 });

    const before = await uiState();
    assert.ok(
      !before.active_panel,
      "no panel should be active before frobbing the elevator button",
    );
    await game.screenshot("elevator-before-frob.png");

    // --- Frob: the elevator panel must open, bound to the button ---
    await game.entities.sendMessage(buttonId, { type: "Frob" });
    await game.step({ frames: 5 });

    const opened = await uiState();
    assert.ok(
      opened.active_panel,
      `frobbing the elevator button should open its MFD panel (got ${JSON.stringify(opened)})`,
    );
    assert.equal(
      opened.active_panel.entity_id,
      buttonId,
      "the active panel should be bound to the elevator button entity",
    );

    // --- No power: retail shows only POWER.PCX, with no floor controls. ---
    assert.ok(
      opened.active_panel.elements.some(
        (element) => element.texture === "iface/power.pcx",
      ),
      "an unset ElevState should draw the archive-qualified no-power panel",
    );
    assert.deepEqual(
      opened.active_panel.elements
        .filter((element) => element.kind === "button")
        .map((element) => element.label),
      ["close"],
      "an unpowered elevator should expose no floor buttons",
    );
    await game.screenshot("elevator-panel-unpowered.png");

    // --- Partial power: decks 1..=3 work; decks 4..=5 remain blocked. ---
    await game.quests.set("ElevState", "incomplete");
    await game.step({ frames: 5 });
    const partial = await uiState();
    assert.ok(
      partial.active_panel,
      "panel should stay open across the power change",
    );
    assert.ok(
      floorButton(partial, ENGINEERING),
      "with partial power, Engineering (deck 1) should be available",
    );
    assert.ok(
      floorButton(partial, HYDROPONICS),
      "with partial power, Hydroponics (deck 3) should be available",
    );
    assert.ok(
      !floorButton(partial, OPERATIONS),
      "with partial power, Operations (deck 4) should remain sealed",
    );
    assert.ok(
      !floorButton(partial, MED_SCI),
      "the current floor should remain inert (not a clickable button)",
    );
    await game.screenshot("elevator-panel-gated.png");

    // --- Full power and click Engineering: transition to eng1.mis. ---
    await game.quests.set("ElevState", "complete");
    await game.step({ frames: 5 });
    const fullyPowered = await uiState();
    const engineering = floorButton(fullyPowered, ENGINEERING);
    assert.ok(
      engineering,
      "full power should retain the Engineering floor button",
    );

    assert.equal(
      (await game.info()).mission,
      "medsci1.mis",
      "should still be on medsci1 before clicking a floor",
    );

    const [rx, ry, rw, rh] = engineering.screen_rect;
    await game.input.set("pointer.position", [rx + rw / 2, ry + rh / 2]);
    await game.step({ frames: 2 }); // hover (edge detection needs a prior unpressed frame)
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 10 });

    assert.equal(
      (await game.info()).mission,
      "eng1.mis",
      "clicking the Engineering floor should transition to eng1.mis",
    );
    await game.screenshot("elevator-arrived-eng1.png");
  },
);
