import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanelPose, UiState } from "../src/index.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// The VR cyber interface's pointer bridge (slice 3): a controller ray meets
// the anchored panel, and where it lands drives the SAME host the flat mouse
// drives - hover, the cursor-is-the-item drag, and (on the squeeze) the
// grab-to-hand path loot panels already use.
//
// Negative-first: on the parent the interface has no pointer at all - /v1/ui
// reports no `panel_pose` and no `pointer`, so aiming at a slot changes
// nothing and every assertion below fails there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8581);

const center = (el: UiElement): [number, number] => [
  el.rect[0] + el.rect[2] / 2,
  el.rect[1] + el.rect[3] / 2,
];

async function openInterface(game: GameServer): Promise<UiState> {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  return game.ui.state();
}

/** The strip element bound to `entityId`, or undefined once it has left the grid. */
function slotFor(ui: UiState, entityId: number): UiElement | undefined {
  return ui.strip?.elements.find((el) => el.entity_id === entityId);
}

function requirePanel(ui: UiState): UiPanelPose {
  assert.ok(ui.panel_pose, "the VR cyber interface must report its panel pose");
  return ui.panel_pose;
}

/** Release, then pull: a clean rising edge on the trigger. */
async function clickAt(
  game: GameServer,
  panel: UiPanelPose,
  canvas: [number, number],
): Promise<void> {
  await aimVrHandAtCanvas(game, panel, canvas, { trigger: 0 });
  await game.step({ frames: 2 });
  await aimVrHandAtCanvas(game, panel, canvas, { trigger: 1 });
  await game.step({ frames: 2 });
  await aimVrHandAtCanvas(game, panel, canvas, { trigger: 0 });
  await game.step({ frames: 2 });
}

test(
  "a VR controller ray hovers, clicks and drags on the cyber-interface canvas",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const wrench = await game.player.spawnItem("Wrench");
    let ui = await openInterface(game);
    assert.equal(ui.mode, "use");
    const panel = requirePanel(ui);
    assert.deepEqual(panel.canvas, [640, 480], "the panel presents the shared canvas");

    // Park the left controller off the panel so this scenario is about one
    // hand. Per-hand arbitration means an idle hand resting on the panel is a
    // pointer too - which the "aiming away" step below would otherwise pick up.
    await aimVrHandAtCanvas(game, panel, [320, 240], {
      hand: "left",
      facing: "away",
    });

    const slot = slotFor(ui, wrench.entity_id);
    assert.ok(slot, "the provisioned wrench must occupy a strip slot");
    const slotCenter = center(slot);

    // Hover: the ray must land on the pixels the interface laid the slot out
    // at - the same canvas both presentations render.
    await aimVrHandAtCanvas(game, panel, slotCenter);
    await game.step({ frames: 3 });
    ui = await game.ui.state();
    assert.ok(ui.pointer?.canvas, "the ray must report a canvas hit");
    const [hx, hy] = ui.pointer.canvas;
    assert.ok(
      Math.abs(hx - slotCenter[0]) < 4 && Math.abs(hy - slotCenter[1]) < 4,
      `the ray must land on the slot (aimed ${slotCenter}, hit [${hx}, ${hy}])`,
    );
    assert.equal(ui.cursor, null, "hovering alone must not lift the item");

    // Aiming away: no canvas hit, and that hand is a world hand again.
    await aimVrHandAtCanvas(game, panel, [320, 240], { facing: "away" });
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).pointer?.canvas ?? null,
      null,
      "a ray pointed away from the panel must not report a canvas hit",
    );

    // Click: the trigger is the LMB analog - it lifts the item onto the cursor.
    await clickAt(game, panel, slotCenter);
    ui = await game.ui.state();
    assert.equal(
      ui.cursor?.entity_id,
      wrench.entity_id,
      "a trigger pull on a slot must lift the item onto the cursor",
    );
    assert.equal(
      slotFor(ui, wrench.entity_id),
      undefined,
      "the lifted item leaves the strip grid while it rides the cursor",
    );

    // Drag + place: a click on an empty slot puts it down again, and the item
    // reappears in the grid.
    const emptySlot: [number, number] = [slotCenter[0] + 105, slotCenter[1]];
    await clickAt(game, panel, emptySlot);
    ui = await game.ui.state();
    assert.equal(ui.cursor, null, "clicking an empty slot must place the item");
    assert.ok(
      slotFor(ui, wrench.entity_id),
      "the placed item must be back in the strip grid",
    );

    // Empty panel space is still the interface, not the 3D view: a click there
    // must not throw the item into the world.
    await clickAt(game, panel, slotCenter);
    assert.equal((await game.ui.state()).cursor?.entity_id, wrench.entity_id);
    await clickAt(game, panel, [400, 400]);
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      wrench.entity_id,
      "a click on empty panel space must keep the held item",
    );
    // Put it back so the grab below has a slot to take from.
    await clickAt(game, panel, slotCenter);
    ui = await game.ui.state();
    assert.equal(ui.cursor, null);
  },
);

test(
  "a squeeze on an inventory slot pulls the item into that VR hand",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const wrench = await game.player.spawnItem("Wrench");
    const ui = await openInterface(game);
    const panel = requirePanel(ui);
    const slot = slotFor(ui, wrench.entity_id);
    assert.ok(slot, "the provisioned wrench must occupy a strip slot");

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      null,
      "the hand starts empty",
    );

    // Grab-to-hand parity: squeeze on the slot, exactly as a hand takes an
    // item out of a loot panel today. (That the grab lands in the *pointing*
    // hand rather than a hardcoded one is covered by the unit test
    // `a_squeeze_on_a_slot_reaches_the_strip_as_a_left_hand_grab`; /v1/info
    // only reports the right hand's held entity.)
    await aimVrHandAtCanvas(game, panel, center(slot), { squeeze: 0 });
    await game.step({ frames: 3 });
    await aimVrHandAtCanvas(game, panel, center(slot), { squeeze: 1 });
    await game.step({ frames: 8 });

    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.entity_id,
      "a squeeze on the slot must pull the item into the pointing hand",
    );
    assert.equal(
      slotFor(await game.ui.state(), wrench.entity_id),
      undefined,
      "the grabbed item must leave the backpack grid",
    );

    // The item stays held while the squeeze is held: the UI arbitration masks
    // the squeeze only while the hand is empty, so taking an item cannot then
    // read as a release and drop it.
    await game.step({ frames: 20 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      wrench.entity_id,
      "the taken item must stay held while the squeeze is held",
    );
  },
);
