import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, UiElement, Vec3 } from "../src/types.js";
import { teleportVerified } from "./helpers/teleport.js";
import { add, aimVrHandAt, quatRotate } from "./helpers/vr-hand.js";

// Keycards, cyber modules, nanite piles and audio/data logs are collected, not
// carried. This covers the two gestures that used to acquire them the wrong
// way, on authentic medsci1 data:
//
//   1. A VR SQUEEZE on a loot panel's disc. Before, `panel_grab_effect` made
//      the keycard exception only, so squeezing audio log 251 out of
//      Invulnerable 734's panel pulled the disc into the hand as a physical
//      prop and its `(deck 2, log 14)` entry was never filed.
//   2. A FLAT CLICK on a strip item. The host owns strip clicks as the
//      cursor-is-the-item drag, so a click never reached ContainerGui's frob:
//      an always-collected item already in the backpack - which only an old
//      save, made before its category collected on pickup, can produce - rode
//      the cursor instead of being collected. (#1097 review finding M2.)
//
// The frob paths (world frob/squeeze, the take arm on a disc or module) already
// collected and stay covered by their own tests; the unit matrix in
// `scripts::gui::container` and `mission::flat_ui_host` records every cell.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** medsci1: Invulnerable 734 --Contains--> Audio Log 251 (deck 2, log 14). */
const LOOT_NPC = 734;
const CONTAINED_LOG = 251;
const LOG_DECK = 2;
const LOG_NUMBER = 14;

/** A real pile archetype - `is_always_collected` excludes the FakeNanites decoy. */
const NANITE_PILE = "Big Nanite Pile";

const GUI_PIXEL_TO_WORLD_SIZE = 1 / 250;
const LOOT_PANEL_SIZE_PX: Vec3 = [188, 296, 0];

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

/**
 * Squeeze one element of the VR loot panel with the production hand, by aiming
 * at the element's own spot on the panel's collider (as `hydro2-vr-keycard`
 * does).
 */
async function squeezeWorldPanelElement(
  game: GameServer,
  element: UiElement,
): Promise<void> {
  // The open panel's colliders are anonymous quads sharing one pose, so aim at
  // the pose rather than at a particular body, and confirm the ray lands on one
  // of them before squeezing.
  const panels = (await game.physics.bodies()).bodies.filter((body) =>
    body.collision_groups.includes("ui"),
  );
  assert.ok(panels.length > 0, "the open loot panel must have a UI collider to aim at");
  const [x, y, width, height] = element.screen_rect;
  const u = x + width / 2;
  const v = y + height / 2;
  const panelSize: Vec3 = [
    LOOT_PANEL_SIZE_PX[0] * GUI_PIXEL_TO_WORLD_SIZE,
    LOOT_PANEL_SIZE_PX[1] * GUI_PIXEL_TO_WORLD_SIZE,
    0,
  ];
  const local: Vec3 = [panelSize[0] * (0.5 - u), panelSize[1] * (0.5 - v), 0];
  const target = add(panels[0].position, quatRotate(panels[0].rotation, local));
  const aim = await aimVrHandAt(game, target, 0.35);
  const hit = await game.raycast({
    start: aim.start,
    end: aim.target,
    collision_groups: ["ui"],
    max_distance: 1,
  });
  assert.ok(
    panels.some((panel) => panel.entity_id === hit.entity_id),
    "the production hand ray must land on the panel",
  );

  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 4 });
  await game.input.set("right_hand.squeeze", 0);
  await game.step({ frames: 8 });
}

test(
  "medsci1 VR: squeezing a disc out of a loot panel files the log instead of holding it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8631),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 5 });

    const host = only(await game.entities.byTemplate(LOOT_NPC), `medsci1 ${LOOT_NPC}`);
    const disc = only(await game.entities.byTemplate(CONTAINED_LOG), `audio log ${CONTAINED_LOG}`);
    assert.deepEqual(
      (await game.info()).player.collected_logs,
      [],
      "a fresh medsci1 character has filed no logs",
    );

    // Open the real loot panel by frobbing its host (an invulnerable posed NPC
    // is lootable), then squeeze the disc's own button.
    await teleportVerified(game, {
      x: host.position[0] + 1.0,
      y: host.position[1] + 0.5,
      z: host.position[2] + 1.0,
    });
    await game.entities.sendMessage(host.id, { type: "Frob" });
    await game.step({ frames: 8 });
    const panel = (await game.ui.state()).active_panel;
    assert.equal(panel?.entity_id, host.id, `${LOOT_NPC} must open its loot panel`);
    const discElement = panel.elements.find(
      (element) => element.kind === "button" && element.entity_id === disc.id,
    );
    assert.ok(discElement, "the loot panel must expose a button for the contained disc");

    await squeezeWorldPanelElement(game, discElement);

    assert.deepEqual(
      (await game.info()).player.collected_logs,
      [{ deck: LOG_DECK, log: LOG_NUMBER, read: false }],
      "the squeeze must file the disc's log in the PDA",
    );
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      null,
      "a collected disc must never come to rest in the hand",
    );
    assert.equal(
      (await game.player.inventory()).items.find((item) => item.entity_id === disc.id),
      undefined,
      "a collected disc must not occupy an inventory slot either",
    );
  },
);

test(
  "medsci1 flat: clicking an already-inventoried nanite pile in the strip credits it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8632),
    });
    await game.step({ frames: 5 });

    // Stage the pile directly in the backpack, bypassing every acquisition path
    // - the state an old save leaves behind, and the one the strip click had no
    // way to recover from.
    const pile = await game.player.spawnItem(NANITE_PILE);
    const stack = Number(
      (await game.entities.detail(pile.entity_id)).properties.find(
        (property) => property.name === "StackCount",
      )?.value,
    );
    assert.ok(stack > 0, "the staged pile must carry an authored stack count");
    assert.equal(
      (await game.info()).player.stats?.nanites ?? 0,
      0,
      "a fresh medsci1 character starts with no stat nanites",
    );

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 8 });
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use", "ToggleUseMode must raise the inventory strip");
    const slot = ui.strip?.elements.find((element) => element.entity_id === pile.entity_id);
    assert.ok(slot, "the staged pile must occupy a strip slot");

    const [x, y, width, height] = slot.screen_rect;
    const center: [number, number] = [x + width / 2, y + height / 2];
    await game.input.set("pointer.position", center);
    await game.step({ frames: 2 }); // an unpressed frame first: the click is an edge
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 6 });

    assert.equal(
      (await game.info()).player.stats?.nanites,
      stack,
      "clicking the pile must credit its stack to the nanite stat",
    );
    const after = await game.ui.state();
    assert.equal(after.cursor, null, "a collected pile must never ride the cursor");
    assert.equal(
      after.strip?.elements.find((element) => element.entity_id === pile.entity_id),
      undefined,
      "the collected pile must leave the strip grid",
    );
    assert.equal(
      (await game.player.inventory()).items.find((item) => item.entity_id === pile.entity_id),
      undefined,
      "the collected pile must leave the backpack",
    );
  },
);
