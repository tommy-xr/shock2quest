import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiPanelPose, UiState } from "../src/index.js";
import {
  canvasCenter as center,
  clickCanvasWithRay,
  requirePanelPose as requirePanel,
} from "./helpers/ui.js";
import { aimVrHandAtCanvas, dot, normalize } from "./helpers/vr-hand.js";

// The VR cyber interface's pointer bridge (slice 3): a controller ray meets
// the anchored panel, and where it lands drives the SAME host the flat mouse
// drives - hover, the cursor-is-the-item drag, and (on the squeeze) the
// grab-to-hand path loot panels already use.
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
  "a VR controller ray hovers, clicks and drags on the cyber-interface canvas",
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

    // Click: the trigger is the LMB analog - it lifts the item onto the cursor.
    await clickCanvasWithRay(game, panel, slotCenter);
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
    await clickCanvasWithRay(game, panel, emptySlot);
    ui = await game.ui.state();
    assert.equal(ui.cursor, null, "clicking an empty slot must place the item");
    assert.ok(
      slotFor(ui, wrench.entity_id),
      "the placed item must be back in the strip grid",
    );

    // Empty panel space is still the interface, not the 3D view: a click there
    // must not throw the item into the world.
    await clickCanvasWithRay(game, panel, slotCenter);
    assert.equal((await game.ui.state()).cursor?.entity_id, wrench.entity_id);
    await clickCanvasWithRay(game, panel, [400, 400]);
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      wrench.entity_id,
      "a click on empty panel space must keep the held item",
    );
    // Put it back so the grab below has a slot to take from.
    await clickCanvasWithRay(game, panel, slotCenter);
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

// The VR reading of flat's "click the bare 3D view": with an item riding the
// cursor, a trigger pull with the ray OFF the panel throws it into the world
// along that hand's ray. Negative-first: on the parent an off-panel press is
// ignored, so the cursor keeps the item and no body ever appears.
test(
  "an off-panel trigger pull throws the cursor item into the world",
  { skip: e2eEnabled ? false : "set SHOCK2_E2E=1 to run", timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const wrench = await game.player.spawnItem("Wrench");
    let ui = await openInterface(game);
    const panel = requirePanel(ui);
    await aimVrHandAtCanvas(game, panel, [320, 240], {
      hand: "left",
      facing: "away",
    });
    const slot = slotFor(ui, wrench.entity_id);
    assert.ok(slot, "the provisioned wrench must occupy a strip slot");

    await clickCanvasWithRay(game, panel, center(slot));
    ui = await game.ui.state();
    assert.equal(ui.cursor?.entity_id, wrench.entity_id, "the wrench rides the cursor");

    // The launch velocity is set on the throw frame, so read it one frame in,
    // before gravity and the floor have had a say.
    const launchVelocity = async (entityId: number) => {
      const launched = await game.physics.bodies({ entityId });
      assert.ok(launched.bodies.length > 0, "the throw gives the item a body");
      return normalize(launched.bodies[0].velocity);
    };

    // Throw 1: the RIGHT hand, past the panel's right edge (its ray misses the
    // canvas, so it is a world hand) - aimed forward, into the panel plane.
    const playerBefore = await game.player.position();
    const rightOff: [number, number] = [1000, 240];
    await aimVrHandAtCanvas(game, panel, rightOff, { trigger: 0 });
    await game.step({ frames: 2 });
    assert.equal(
      (await game.ui.state()).cursor?.entity_id,
      wrench.entity_id,
      "an off-panel hand at rest must not disturb the cursor",
    );
    await aimVrHandAtCanvas(game, panel, rightOff, { trigger: 1 });
    await game.step({ frames: 1 });
    const forwardThrow = await launchVelocity(wrench.entity_id);
    await aimVrHandAtCanvas(game, panel, rightOff, { trigger: 0 });
    await game.step({ frames: 10 });

    // Throw 2: lift a second wrench with the right hand (which then rests on
    // the panel, trigger up), and throw it with the LEFT hand turned AWAY from
    // the panel. A throw aimed by the wrong hand - the right one, or the head
    // - would fly forward like the first; the left hand's own ray points the
    // other way, so the two throws must be anti-parallel.
    const second = await game.player.spawnItem("Wrench");
    await game.step({ frames: 2 });
    const secondSlot = slotFor(await game.ui.state(), second.entity_id);
    assert.ok(secondSlot, "the second wrench must occupy a strip slot");
    await clickCanvasWithRay(game, panel, center(secondSlot));
    assert.equal((await game.ui.state()).cursor?.entity_id, second.entity_id);
    await aimVrHandAtCanvas(game, panel, [320, 240], {
      hand: "left",
      facing: "away",
      trigger: 1,
    });
    await game.step({ frames: 1 });
    const backwardThrow = await launchVelocity(second.entity_id);
    assert.ok(
      dot(forwardThrow, backwardThrow) < -0.9,
      `each throw must fly along the ray of the hand that pulled (cos ${dot(forwardThrow, backwardThrow).toFixed(2)})`,
    );
    await aimVrHandAtCanvas(game, panel, [320, 240], {
      hand: "left",
      facing: "away",
      trigger: 0,
    });
    await game.step({ frames: 10 });

    ui = await game.ui.state();
    assert.equal(ui.cursor ?? null, null, "the throw clears the cursor");
    assert.equal(slotFor(ui, wrench.entity_id), undefined, "the thrown wrench leaves the strip");
    const carried = await game.player.inventory();
    assert.ok(
      !carried.items.some((item) => item.entity_id === wrench.entity_id),
      "the thrown wrench is no longer carried",
    );
    const bodies = await game.physics.bodies({ entityId: wrench.entity_id });
    assert.ok(bodies.bodies.length > 0, "the thrown wrench has a physics body");
    const p = bodies.bodies[0].position;
    const dist = Math.hypot(
      p[0] - playerBefore.x,
      p[1] - playerBefore.y,
      p[2] - playerBefore.z,
    );
    assert.ok(
      Number.isFinite(dist) && dist < 10,
      `the thrown wrench lands near the player (dist ${dist.toFixed(2)})`,
    );
  },
);
