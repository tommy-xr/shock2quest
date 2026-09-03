import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState } from "../src/index.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// The inventory bar's mini-frame name line (retail's readout in the blank slot
// between the INVENTORY and EQUIP labels): the display name of whatever the
// player is pointing at, drawn on the SHARED use-mode canvas so flat use mode
// and the VR cyber interface report the same thing.
//
// The name comes from the same `P$ObjName` resolution the HUD brackets label
// with - hence "A basketball", not the symbolic "Basketball".
//
// Negative-first: on the parent `/v1/ui` has no `name_strip` field at all, so
// every assertion below reads `undefined` and fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** A pickup whose template carries a `P$ObjName` (-> "A basketball"). */
const NAMED_ITEM = "Basketball";
const NAMED_ITEM_READOUT = "A basketball";

/** SpawnDebugMonster's template (grunt og-pipe), named "A hybrid". */
const DEBUG_MONSTER_TEMPLATE = -397;

const center = (el: UiElement): [number, number] => [
  el.rect[0] + el.rect[2] / 2,
  el.rect[1] + el.rect[3] / 2,
];

async function openUseMode(game: GameServer): Promise<UiState> {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  return game.ui.state();
}

function slotFor(ui: UiState, entityId: number): UiElement {
  const el = ui.strip?.elements.find((e) => e.entity_id === entityId);
  assert.ok(el, "the provisioned item must occupy a strip slot");
  return el;
}

test(
  "flat use mode names the item under the mouse in the mini-frame",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 30 });

    const item = await game.player.spawnItem(NAMED_ITEM);
    let ui = await openUseMode(game);
    assert.equal(ui.mode, "use");
    assert.equal(ui.name_strip, null, "an idle pointer over nothing names nothing");

    // The mouse over the item's slot: the readout names it.
    const [sx, sy, sw, sh] = slotFor(ui, item.entity_id).screen_rect;
    await game.input.set("pointer.position", [sx + sw / 2, sy + sh / 2]);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).name_strip,
      NAMED_ITEM_READOUT,
      "the slot under the mouse is named",
    );

    // Lifting it keeps the readout on the item riding the cursor, wherever the
    // mouse goes.
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 2 });
    ui = await game.ui.state();
    assert.equal(ui.cursor?.entity_id, item.entity_id, "the click must lift it");
    await game.input.set("pointer.position", [0.5, 0.85]);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).name_strip,
      NAMED_ITEM_READOUT,
      "the cursor IS the item, so it stays named off the grid",
    );

    // Leaving use mode takes the bar - and its readout - with it.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const shooter = await game.ui.state();
    assert.equal(shooter.mode, "shooter");
    assert.equal(shooter.name_strip, null, "no inventory bar, no readout");
  },
);

test(
  "the VR cyber interface names the slot under the ray, then the world object off-panel",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const item = await game.player.spawnItem(NAMED_ITEM);
    const ui = await openUseMode(game);
    assert.equal(ui.mode, "use");
    const panel = ui.panel_pose;
    assert.ok(panel, "the VR interface must report its panel pose");

    // Park the left hand off the panel: per-hand arbitration means an idle
    // hand on the panel is a pointer too.
    await aimVrHandAtCanvas(game, panel, [320, 240], {
      hand: "left",
      facing: "away",
    });

    // The ray on the item's slot: the same readout the mouse drives, on the
    // same canvas.
    await aimVrHandAtCanvas(game, panel, center(slotFor(ui, item.entity_id)));
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).name_strip,
      NAMED_ITEM_READOUT,
      "the slot under the VR ray is named",
    );

    // Off the panel: the readout falls back to the world object the hand is
    // pointing at - a monster spawned in front of the player.
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 60 });
    const monster = (await game.entities.list({ limit: 400 })).entities.find(
      (e) => e.template_id === DEBUG_MONSTER_TEMPLATE,
    );
    assert.ok(monster, "the debug monster must exist in the world");

    await aimVrHandAtCanvas(game, panel, [320, 240], { facing: "away" });
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).pointer?.canvas ?? null,
      null,
      "the ray is off the panel",
    );

    await aimVrHandAt(game, monster.position);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).name_strip,
      "A hybrid",
      "pointing at a world object names it, panel or no panel",
    );
  },
);
