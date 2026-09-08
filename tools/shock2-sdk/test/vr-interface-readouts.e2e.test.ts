import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import type { UiElement, UiState, Vec3 } from "../src/index.js";
import {
  canvasCenter,
  clickCanvasWithRay,
  requirePanelPose,
} from "./helpers/ui.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// The VR cyber interface carries the use-mode HUD readouts: the expanded
// BIOFULL (health/psi) and AMMOFULL (ammo/psi power) panels along the bottom
// of the shared 640x480 canvas, at the flat use-mode rects - and their SETTING
// / RELOAD / cycle / psi controls are clickable by the controller ray, which is
// what gives the settings and psi MFDs a way in from VR.
//
// Negative-first: on the parent branch the readouts are drawn only by the FLAT
// HUD (which VR never renders) and `set_readout_buttons` is only called in the
// flat branch, so in VR `/v1/ui` reports no readout elements and no readout
// controls at all - every assertion below fails there.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The debug pistol: a magazine, two fire modes, and more than one ammo type. */
const PISTOL = -17;
/** The Psi Amp player weapon. */
const PSI_AMP = -247;

function drawn(ui: UiState, label: string): UiElement {
  const found = ui.readout_elements.find((e) => e.label === label);
  assert.ok(
    found,
    `expected a drawn "${label}" element, got ${ui.readout_elements.map((e) => e.label)}`,
  );
  return found;
}

function control(ui: UiState, label: string): UiElement {
  const found = ui.readout.find((e) => e.label === label);
  assert.ok(
    found,
    `expected a "${label}" readout control, got ${ui.readout.map((e) => e.label)}`,
  );
  return found;
}

/** Take `template` in the VR right hand, then open the cyber interface. */
async function holdAndOpen(
  game: GameServer,
  template: number,
): Promise<UiState> {
  const item = (await game.entities.list()).entities.find(
    (e) => e.template_id === template,
  );
  assert.ok(item, `the scene should stock template ${template}`);
  await aimVrHandAt(game, item.position as Vec3, 0.35);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    item.id,
    "the VR right hand must hold the item",
  );

  await game.input.trigger("ToggleUseMode");
  await game.step({ frames: 8 });
  const ui = await game.ui.state();
  assert.equal(ui.mode, "use");
  // Park the gun hand off the panel: per-hand arbitration would otherwise let
  // it be the pointer, and this scenario is about the free hand's ray.
  await aimVrHandAtCanvas(game, requirePanelPose(ui), [320, 240], {
    hand: "right",
    squeeze: 1,
    facing: "away",
  });
  await game.step({ frames: 3 });
  return game.ui.state();
}

test(
  "the VR cyber interface shows the use-mode readouts at the flat rects",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: e2ePort(0, "SHOCK2_E2E_VR_READOUT_PORT"),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    // Shooter mode carries no interface readouts - there is no canvas up.
    const shooter = await game.ui.state();
    assert.deepEqual(shooter.readout_elements, []);
    assert.deepEqual(shooter.readout, []);

    const ui = await holdAndOpen(game, PISTOL);
    const pose = requirePanelPose(ui);
    assert.deepEqual(pose.canvas, [640, 480], "the panel presents the whole canvas");

    // Both readouts, at the anchors flat use mode draws them at.
    assert.deepEqual(
      drawn(ui, "biofull").rect,
      [2, 414, 260, 64],
      "BIOFULL keeps the original meters anchor",
    );
    assert.deepEqual(
      drawn(ui, "ammofull").rect,
      [378, 414, 260, 64],
      "AMMOFULL expands left from the compact gauge",
    );

    // ...and every control the pistol offers is on it, clickable.
    assert.deepEqual(
      ui.readout.map((e) => e.label),
      ["gun_setting", "reload", "cycle_ammo", "system_menu"],
    );
    assert.equal(
      control(ui, "gun_setting").text,
      (await game.info()).player.wielded_gun_setting_header,
      "the button is labeled with the mode the gun is actually in",
    );

    // The TOGGLING frame itself must already agree with what is drawn: the flat
    // HUD skips whatever the interface owns, reading `use_mode` independently,
    // so a frame where the two disagree shows either no readouts at all or the
    // compact pair with the expanded pair painted over it.
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 1 });
    assert.deepEqual(
      (await game.ui.state()).readout_elements,
      [],
      "the frame that closes the interface must drop the readouts with it",
    );
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 1 });
    assert.ok(
      (await game.ui.state()).readout_elements.length > 0,
      "the frame that opens the interface must already carry the readouts",
    );
  },
);

test(
  "the VR pointer clicks the readout's controls",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: e2ePort(1, "SHOCK2_E2E_VR_READOUT_PORT"),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    let ui = await holdAndOpen(game, PISTOL);
    const pose = requirePanelPose(ui);

    // SETTING docks the weapon settings MFD - the entry point #1254 deferred
    // to this PR.
    assert.equal(ui.active_panel, null, "nothing is docked before the click");
    await clickCanvasWithRay(
      game,
      pose,
      canvasCenter(control(ui, "gun_setting")),
      "left",
    );
    ui = await game.ui.state();
    assert.equal(
      ui.active_panel?.name,
      "Weapon Settings",
      "the ray on SETTING must dock the settings MFD",
    );

    // The cycle arrow steps the wielded gun's ammo type.
    const before = (await game.info()).player.wielded_ammo_type;
    await clickCanvasWithRay(
      game,
      pose,
      canvasCenter(control(ui, "cycle_ammo")),
      "left",
    );
    await game.step({ frames: 5 });
    assert.notEqual(
      (await game.info()).player.wielded_ammo_type,
      before,
      "the ray on the cycle arrow must change the ammo type",
    );
  },
);

test(
  "the amp's psi readout opens the power MFD from the VR interface",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: e2ePort(2, "SHOCK2_E2E_VR_READOUT_PORT"),
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    const ui = await holdAndOpen(game, PSI_AMP);
    const pose = requirePanelPose(ui);
    // The amp has no clip: the gauge shows its discipline and the selector
    // arrows in place of the gun controls.
    assert.deepEqual(drawn(ui, "ammofull").rect, [378, 414, 260, 64]);
    assert.ok(
      ui.readout.some((e) => e.label === "psi_select"),
      `the psi readout should be clickable: ${ui.readout.map((e) => e.label)}`,
    );

    await clickCanvasWithRay(
      game,
      pose,
      canvasCenter(control(ui, "psi_select")),
      "left",
    );
    assert.equal(
      (await game.ui.state()).active_panel?.name,
      "Psi Powers",
      "the ray on the discipline name must dock the psi powers MFD",
    );
  },
);
