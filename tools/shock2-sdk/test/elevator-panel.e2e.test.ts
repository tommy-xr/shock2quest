import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";

// End-to-end test for the flat-mode elevator MFD (projects/flat-ui-panels.md
// §2, PR A - "flat UI 6a"): frobbing the medsci1 Master Elevator Button opens
// ElevatorGui in the left MFD, whose floor buttons are now semantically
// labeled from MISC.STR (`ElevLevel<n>`), the current deck is lit + inert, and
// floors gate on the `ElevState` quest bit. Clicking an available floor
// transitions the game to that mission (marker StartLoc 22).
//
// Entity discovery is by NAME at run time (runtime entity ids are NOT stable
// across launches): the "Master Elevator Button" (medsci1 mission id 1041,
// script ElevatorButton). The floor labels/decks are global data (the gamesys
// `Elev` file-var), so the button carries no floor links.
//
// Negative-first: on main the floor buttons render with `label: null` (the
// host only knew the keypad's art-derived labels), so the "button labeled
// 'Engineering (1)'" assertion fails; the fix adds the generic button-label
// field + MISC.STR floor names.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The five floor labels ElevatorGui reads from MISC.STR (deck 1..5). Med / Sci
// (deck 2) is the current floor in medsci1 - lit + inert, never a button.
const ENGINEERING = "Engineering (1)";
const OPERATIONS = "Operations (4)";

const floorButton = (state: UiState, label: string): UiElement | undefined =>
  state.active_panel?.elements.find(
    (e) => e.kind === "button" && e.label === label,
  );

test(
  "flat elevator MFD: labeled floors, current-floor inert, ElevState gating, click transitions",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8163),
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

    // --- Floor buttons must be semantically labeled (THE fix; negative on main) ---
    assert.ok(
      floorButton(opened, ENGINEERING),
      `panel should expose a floor button labeled "${ENGINEERING}" (on main labels are null)`,
    );
    assert.ok(
      floorButton(opened, OPERATIONS),
      `panel should expose a floor button labeled "${OPERATIONS}"`,
    );
    // The current deck (Med / Sci, deck 2) is lit + inert - never a clickable
    // button.
    assert.ok(
      !floorButton(opened, "Med / Sci (2)"),
      "the current floor should be inert (not a clickable button)",
    );
    await game.screenshot("elevator-panel-open.png");

    // --- Gating: ElevState = INCOMPLETE seals deck 1 (Engineering) ---
    await game.quests.set("ElevState", "incomplete");
    await game.step({ frames: 5 });
    const gated = await uiState();
    assert.ok(gated.active_panel, "panel should stay open across the gate change");
    assert.ok(
      !floorButton(gated, ENGINEERING),
      "with ElevState=incomplete, Engineering (deck 1) must be sealed (no button)",
    );
    assert.ok(
      floorButton(gated, OPERATIONS),
      "with ElevState=incomplete, decks >= 2 (e.g. Operations) stay available",
    );
    await game.screenshot("elevator-panel-gated.png");

    // --- Ungate and click Engineering: the game transitions to eng1.mis ---
    await game.quests.set("ElevState", "unknown");
    await game.step({ frames: 5 });
    const ungated = await uiState();
    const engineering = floorButton(ungated, ENGINEERING);
    assert.ok(
      engineering,
      "ungating should restore the Engineering floor button",
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
