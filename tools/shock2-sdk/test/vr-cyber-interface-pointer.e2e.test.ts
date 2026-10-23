import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanelPose, UiState } from "../src/index.js";
import { isolatedDataRoot } from "./helpers/isolated-data-root.js";
import {
  canvasCenter as center,
  clickCanvasWithRay,
  requirePanelPose as requirePanel,
} from "./helpers/ui.js";
import { aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// The VR cyber interface's pointer bridge (slice 3): a controller ray meets
// the anchored panel, and where it lands drives the SAME host the flat mouse
// drives for hover and buttons. Inventory movement uses a physical squeeze
// and release, like the grab-to-hand path on loot panels.
//
// Negative-first: on the parent the interface has no pointer at all - /v1/ui
// reports no `panel_pose` and no `pointer`, so aiming at a slot changes
// nothing and every assertion below fails there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function openInterface(game: GameServer): Promise<UiState> {
  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 5 });
  return game.ui.state();
}

/** The strip element bound to `entityId`, or undefined once it has left the grid. */
function slotFor(ui: UiState, entityId: number): UiElement | undefined {
  return ui.strip?.elements.find((el) => el.entity_id === entityId);
}

test(
  "a VR controller ray hovers and moves inventory items with a grab",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
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

    // VR inventory uses physical hands; a trigger on this non-usable item
    // does not create a flat cursor or remove it from the backpack.
    await clickCanvasWithRay(game, panel, slotCenter);
    ui = await game.ui.state();
    assert.equal(ui.cursor, null);
    assert.ok(slotFor(ui, wrench.entity_id));

    await aimVrHandAtCanvas(game, panel, slotCenter, { squeeze: 1 });
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, wrench.entity_id);
    assert.equal(slotFor(await game.ui.state(), wrench.entity_id), undefined);

    // Move to a different free strip cell while gripping, then release to
    // place the same physical item back in the backpack.
    const emptySlot: [number, number] = [slotCenter[0] + 105, slotCenter[1]];
    await aimVrHandAtCanvas(game, panel, emptySlot, { squeeze: 1 });
    await game.step({ frames: 3 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    ui = await game.ui.state();
    const placed = slotFor(ui, wrench.entity_id);
    assert.ok(placed, "the released item returns to the strip grid");
    assert.ok(center(placed)[0] > slotCenter[0], "the item moved to the new column");
    assert.equal(ui.cursor, null, "physical placement creates no flat cursor");

    const inventory = await game.player.inventory();
    await clickCanvasWithRay(game, panel, [400, 400]);
    assert.deepEqual(await game.player.inventory(), inventory,
      "empty panel chrome must not throw an inventory item into the world");
  },
);

test(
  "a squeeze on an inventory slot pulls the item into that VR hand",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run" },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
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

/** Launch medsci1 in VR on a private data root, with developer mode on or off. */
async function launchWithDeveloperMode(developer: boolean, flags: string[]) {
  const data = isolatedDataRoot([
    "developer-mode", "vr-grip-overrides.json", "saves", "recordings", "settings.json", "user-settings.json",
  ]);
  if (developer) writeFileSync(join(data.root, "developer-mode"), "Developer mode enabled\n");
  const game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: ["--vr", ...flags] });
  await game.step({ frames: 30 });
  return { data, game };
}

const isPose = (el: UiElement) => el.label === "pose_held_item";

test(
  "the cyber interface's Pose button needs multimodal and developer mode, and opens the grip pose workbench",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 300_000 },
  async () => {
    // Developer mode without simultaneous hands and controllers: no button.
    {
      const { data, game } = await launchWithDeveloperMode(true, []);
      using _data = data;
      await using _game = game;
      const ui = await openInterface(game);
      assert.equal(ui.mode, "use");
      assert.ok(!ui.readout.some(isPose), "no Pose without multimodal");
      assert.ok(!ui.readout_elements.some((el) => el.text === "POSE"));
    }
    // Multimodal without developer mode: none either.
    {
      const { data, game } = await launchWithDeveloperMode(false, ["--multimodal"]);
      using _data = data;
      await using _game = game;
      const ui = await openInterface(game);
      assert.ok(!ui.readout_elements.some((el) => el.text === "POSE"), "no Pose without developer mode");
    }

    const { data, game } = await launchWithDeveloperMode(true, ["--multimodal"]);
    using _data = data;
    await using _game = game;
    const wrench = await game.player.spawnItem("Wrench");
    let ui = await openInterface(game);
    // Empty-handed: shown, but not pressable, and says why.
    assert.ok(!ui.readout.some(isPose), "nothing held: not clickable");
    assert.ok(ui.readout_elements.some((el) => el.text === "POSE"));
    assert.ok(ui.readout_elements.some((el) => el.text === "hold an item"));

    // Take the wrench into the right hand with a squeeze on its slot.
    const panel = requirePanel(ui);
    const slot = slotFor(ui, wrench.entity_id)!;
    await aimVrHandAtCanvas(game, panel, center(slot), { squeeze: 0 });
    await game.step({ frames: 3 });
    await aimVrHandAtCanvas(game, panel, center(slot), { squeeze: 1 });
    await game.step({ frames: 8 });
    assert.equal((await game.info()).player.right_hand_entity_id, wrench.entity_id);
    ui = await game.ui.state();
    const pose = ui.readout.find(isPose);
    assert.ok(pose, "holding the wrench, Pose is pressable");
    assert.deepEqual(pose.rect.slice(0, 2), [288, 332], "above MENU");

    // The free left hand's ray presses it: the interface closes and the
    // workbench opens on the wrench.
    await clickCanvasWithRay(game, panel, center(pose), "left");
    await game.step({ frames: 3 });
    assert.notEqual((await game.ui.state()).mode, "use", "the interface closed");
    const posing = (await game.info()).player.hand_grips.find((g) => g.pose_tool);
    assert.equal(posing?.hand, "right", "the workbench poses the right hand");
    assert.equal(posing?.entity_id, wrench.entity_id);
  },
);
