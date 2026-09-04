import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement, UiState, Vec3 } from "../src/index.js";
import { canvasCenter, clickCanvasWithRay, requirePanelPose } from "./helpers/ui.js";
import { aimVrHandAt } from "./helpers/vr-hand.js";
import { cycleToWeapon } from "./helpers/weapon.js";

// The v2 Touch mapping:
//
//   | hand holds | lower (X/A) | upper (Y/B) |
//   |------------|-------------|-------------|
//   | anything   | jump        | -           |
//   | empty/melee/other | jump | last unread log |
//   | gun        | jump        | toggle fire mode |
//   | psi amp    | jump        | open the psi selector |
//
// ...plus the left Menu button, which is now BOTH the way in and out of the
// cyber interface (short press) and the way to the pause menu (held 0.5 s).
//
// Negative-first: on the parent branch the lower buttons open the cyber
// interface (they never jump), the amp's selector is on the LOWER button, the
// Menu button toggles the pause menu on its edge with no short/long split at
// all, and the interface canvas carries no system button - so every assertion
// below fails there.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_BUTTONS_V2_PORT ?? 8742);

/** The debug pistol `DebugCycleWeapon` spawns first. */
const PISTOL = -17;
/** The Psi Amp player weapon. */
const PSI_AMP = -247;

/** How long a hold must run to cross `MENU_LONG_PRESS` (0.5 s at 60 Hz). */
const LONG_PRESS_FRAMES = 40;
/** Comfortably under it. */
const SHORT_PRESS_FRAMES = 6;

async function uiMode(game: GameServer): Promise<string> {
  return (await game.ui.state()).mode;
}

async function press(game: GameServer, action: string): Promise<void> {
  await game.input.trigger(action);
  await game.step({ frames: 5 });
}

/** Hold the Menu button for `frames`, then let it go. */
async function holdMenu(game: GameServer, frames: number): Promise<void> {
  await game.input.hold("MenuButton");
  await game.step({ frames });
  await game.input.release("MenuButton");
  await game.step({ frames: 5 });
}

/** The highest the player's feet reach over the next `frames` frames. */
async function peakHeight(game: GameServer, frames: number): Promise<number> {
  let peak = (await game.player.position()).y;
  for (let i = 0; i < frames; i++) {
    await game.step({ frames: 1 });
    peak = Math.max(peak, (await game.player.position()).y);
  }
  return peak;
}

/** Take `template` in the VR right hand. */
async function grabWeapon(game: GameServer, template: number): Promise<number> {
  // Diffed against the entity list: the debug scenes already stock one of every
  // gun, so a plain template search could hand back the scenery one.
  const weapon = await cycleToWeapon(game, (e) => e.template_id === template);
  await aimVrHandAt(game, weapon.position as Vec3, 0.3);
  await game.input.set("right_hand.squeeze", 1);
  await game.step({ frames: 8 });
  assert.equal(
    (await game.info()).player.right_hand_entity_id,
    weapon.id,
    "the VR right hand must hold the weapon",
  );
  return weapon.id;
}

function control(ui: UiState, label: string): UiElement {
  const found = ui.readout.find((e) => e.label === label);
  assert.ok(
    found,
    `expected a "${label}" readout control, got ${ui.readout.map((e) => e.label)}`,
  );
  return found;
}

test(
  "a lower button jumps - empty-handed, and with a gun in that hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 60 });

    // Grounded and settled: whatever height the player rises to next came from
    // the button.
    const resting = (await game.player.position()).y;
    assert.ok(
      (await peakHeight(game, 20)) - resting < 0.05,
      "the player must be standing still before the button is pressed",
    );

    await game.input.trigger("RightHandLowerButton");
    const empty = await peakHeight(game, 30);
    assert.ok(
      empty > resting + 0.2,
      `right A with an empty hand must jump (rested ${resting}, peaked ${empty})`,
    );
    assert.equal(
      await uiMode(game),
      "shooter",
      "and it must NOT open the cyber interface any more",
    );

    // The whole point of the row: a hand full of gun still jumps.
    await game.step({ frames: 60 });
    await grabWeapon(game, PISTOL);
    await game.step({ frames: 30 });
    const armed = (await game.player.position()).y;
    await game.input.trigger("RightHandLowerButton");
    const withGun = await peakHeight(game, 30);
    assert.ok(
      withGun > armed + 0.2,
      `the gun hand's lower button must still jump (rested ${armed}, peaked ${withGun})`,
    );

    // ...and so does the other hand's, with the gun still held.
    await game.step({ frames: 60 });
    const before = (await game.player.position()).y;
    await game.input.trigger("LeftHandLowerButton");
    assert.ok(
      (await peakHeight(game, 30)) > before + 0.2,
      "the free hand's lower button jumps too",
    );
  },
);

test(
  "the Menu button jacks in on a short press and pauses on a long one",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    assert.equal(await uiMode(game), "shooter");

    await holdMenu(game, SHORT_PRESS_FRAMES);
    assert.equal(
      await uiMode(game),
      "use",
      "a short Menu press must open the cyber interface",
    );
    assert.equal((await game.info()).paused, false, "and must not pause");

    await holdMenu(game, SHORT_PRESS_FRAMES);
    assert.equal(await uiMode(game), "shooter", "a second short press closes it");

    // The long press: the pause menu opens the moment the threshold is
    // crossed, and the release that follows must NOT then toggle the
    // interface - the defect a press-toggle plus a hold would have.
    await game.input.hold("MenuButton");
    await game.step({ frames: LONG_PRESS_FRAMES });
    assert.equal(
      (await game.info()).paused,
      true,
      "holding the Menu button past the threshold must open the pause menu",
    );
    await game.input.release("MenuButton");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).paused,
      true,
      "the release must be swallowed, not close the menu it just opened",
    );

    // Leaving the menu the same way, and the interface was never toggled.
    await holdMenu(game, SHORT_PRESS_FRAMES);
    assert.equal((await game.info()).paused, false, "a short press closes it again");
    assert.equal(
      await uiMode(game),
      "shooter",
      "the whole long press must never have opened the interface",
    );

    // A single-shot injection (no hold at all) is the ordinary short press.
    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "use");
    await press(game, "MenuButton");
    assert.equal(await uiMode(game), "shooter");
  },
);

test(
  "an upper button belongs to the weapon in its own hand",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 2,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await grabWeapon(game, PISTOL);

    const before = (await game.info()).player.wielded_gun_setting_header;
    await press(game, "RightHandUpperButton");
    assert.notEqual(
      (await game.info()).player.wielded_gun_setting_header,
      before,
      "right B on the gun hand must toggle that gun's fire mode",
    );

    // The free hand's upper button is still the log reader, not the gun's.
    const header = (await game.info()).player.wielded_gun_setting_header;
    await press(game, "LeftHandUpperButton");
    assert.equal(
      (await game.info()).player.wielded_gun_setting_header,
      header,
      "the free hand's button must not reach the other hand's gun",
    );
  },
);

test(
  "the psi amp's upper button opens the power selection MFD",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: basePort + 3,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });
    await grabWeapon(game, PSI_AMP);

    assert.equal((await game.ui.state()).active_panel, null);
    // The selector moved UP from the lower button, which is jump now.
    await press(game, "RightHandUpperButton");
    await game.step({ frames: 5 });
    assert.equal(
      (await game.ui.state()).active_panel?.name,
      "Psi Powers",
      "the amp hand's upper button must dock the psi selection MFD",
    );
  },
);

test(
  "the interface's system button opens the pause menu under the VR ray",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: basePort + 4,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    await press(game, "MenuButton");
    const ui = await game.ui.state();
    assert.equal(ui.mode, "use");

    // Drawn where it is clickable: one list feeds both.
    const button = control(ui, "system_menu");
    assert.deepEqual(button.rect, [288, 372, 64, 36]);
    assert.ok(
      ui.readout_elements.some(
        (e) => JSON.stringify(e.rect) === JSON.stringify(button.rect),
      ),
      "the system button must be drawn at the rect it is hit-tested at",
    );

    assert.equal((await game.info()).paused, false);
    await clickCanvasWithRay(
      game,
      requirePanelPose(ui),
      canvasCenter(button),
      "left",
    );
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).paused,
      true,
      "the ray on the system button must open the pause menu",
    );
  },
);
