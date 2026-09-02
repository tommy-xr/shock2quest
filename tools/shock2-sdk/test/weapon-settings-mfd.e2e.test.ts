import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, type UiElement } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { ammoOf, cycleToWeapon } from "./helpers/weapon.js";

// End-to-end coverage for the weapon settings MFD.
//
// The original's SETTING button does not toggle the fire mode: it opens a panel
// in the left MFD slot listing the gun's settings with the description of each,
// highlights the one the gun is in, and switches when the other row is clicked.
// A gun that takes clips also carries an UNLOAD button that returns the
// magazine to the backpack.
//
// Negative-first: on the base branch clicking SETTING cycles the mode directly
// and no panel ever opens, so every assertion below fails.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The synthetic host entity's name, as `/v1/ui` reports it. */
const PANEL_NAME = "Weapon Settings";

async function panelElements(game: GameServer): Promise<UiElement[]> {
  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, "the weapon settings panel should be open");
  assert.equal(panel.name, PANEL_NAME);
  return panel.elements;
}

async function rowButton(game: GameServer, row: 0 | 1): Promise<UiElement> {
  const label = `setting_${row}`;
  const found = (await panelElements(game)).find((e) => e.label === label);
  assert.ok(found, `expected a "${label}" row on the settings panel`);
  return found;
}

/** The drawn `SETSEL` highlight (an image, not the row's own hit target). */
async function highlight(game: GameServer): Promise<UiElement> {
  const found = (await panelElements(game)).filter(
    (e) => e.kind === "image" && (e.texture ?? "").includes("setsel"),
  );
  assert.equal(found.length, 1, "exactly one row is highlighted");
  return found[0];
}

/** Open the settings MFD the way a player does: use mode, then SETTING. */
async function openSettings(game: GameServer): Promise<void> {
  const setting = ((await game.ui.state()).readout ?? []).find(
    (e) => e.label === "gun_setting",
  );
  assert.ok(setting, "the readout should carry a SETTING button");
  await clickUiElement(game, setting);
  await game.step({ frames: 3 });
}

test(
  "SETTING opens a two-row settings panel and a row click switches the fire mode",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    // The pistol is the first weapon DebugCycleWeapon hands out.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "nothing is docked in the MFD slot before SETTING is pressed",
    );
    await openSettings(game);

    // Both of the pistol's fire settings are listed, each reading
    // "{header}: {description}".
    const elements = await panelElements(game);
    const text = elements
      .filter((e) => e.kind === "text")
      .map((e) => e.text ?? "")
      .join(" ");
    assert.match(text, /NORM:/, `row 0 names the normal mode: ${text}`);
    assert.match(text, /BURST:/, `row 1 names the burst mode: ${text}`);
    assert.match(text, /Pistol/, "the panel names the gun");
    assert.match(text, /Modification level/i, "the panel reports the mod level");

    // The gun starts in mode 0, so the highlight sits on row 0 - exactly over
    // it, since the highlight and the click target are the same rect.
    assert.deepEqual(
      (await highlight(game)).rect,
      (await rowButton(game, 0)).rect,
      "the highlight covers the current setting's row",
    );

    await clickUiElement(game, await rowButton(game, 1));
    await game.step({ frames: 3 });
    const player = (await game.info()).player;
    assert.equal(player.wielded_gun_setting, 1, "clicking row 1 selects setting 1");
    assert.equal(player.wielded_gun_setting_header, "BURST");
    assert.deepEqual(
      (await highlight(game)).rect,
      (await rowButton(game, 1)).rect,
      "and the highlight follows it",
    );

    // Clicking the row the gun is already in is idempotent.
    await clickUiElement(game, await rowButton(game, 1));
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.wielded_gun_setting, 1);
  },
);

test(
  "UNLOAD returns the magazine to the backpack, and an energy weapon has none",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const pistol = await cycleToWeapon(game, (e) => (e.name ?? "") === "Pistol");
    const loaded = ammoOf(await game.entities.detail(pistol.id));
    assert.ok(loaded > 0, "the bench hands out a loaded pistol");

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await openSettings(game);

    const unload = (await panelElements(game)).find((e) => e.label === "unload");
    assert.ok(unload, "a gun that takes clips can eject its magazine");
    await clickUiElement(game, unload);
    await game.step({ frames: 10 });

    assert.equal(
      ammoOf(await game.entities.detail(pistol.id)),
      0,
      "UNLOAD empties the magazine",
    );
    const clips = (await game.player.inventory()).items.filter((item) =>
      (item.name ?? "").includes("Clip"),
    );
    assert.ok(clips.length > 0, `the rounds came back as a clip: ${JSON.stringify(clips)}`);

    // ...and with the magazine now empty the button that empties it is gone,
    // rather than sitting there doing nothing.
    assert.ok(
      !(await panelElements(game)).some((e) => e.label === "unload"),
      "an empty magazine offers no UNLOAD",
    );

    // The laser recharges - it has no magazine to eject - so it shows no
    // UNLOAD, but it still lists both of its fire settings.
    await cycleToWeapon(game, (e) => (e.name ?? "") === "Laser Pistol");
    await game.step({ frames: 5 });
    await openSettings(game);
    const laserLabels = (await panelElements(game))
      .map((e) => e.label)
      .filter((label): label is string => typeof label === "string");
    assert.ok(!laserLabels.includes("unload"), `no UNLOAD on the laser: ${laserLabels}`);
    assert.ok(laserLabels.includes("setting_0") && laserLabels.includes("setting_1"));
  },
);

test(
  "putting the gun away closes the settings panel",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await openSettings(game);
    await panelElements(game);

    // The panel belongs to one gun: cycling to another dismisses it rather
    // than silently re-pointing at the new weapon.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 10 });
    assert.equal(
      (await game.ui.state()).active_panel,
      null,
      "cycling weapons closes the settings panel",
    );
  },
);
