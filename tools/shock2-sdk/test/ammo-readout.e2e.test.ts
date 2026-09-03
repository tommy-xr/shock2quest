import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, type UiElement } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { ammoOf, cycleToWeapon, fireOnce } from "./helpers/weapon.js";

// End-to-end coverage for the use-mode AMMOFULL readout's controls.
//
// The original's expanded weapon panel carries three gun controls - a SETTING
// button labeled with the current fire mode, a RELOAD button, and the ammo-type
// cycle arrow - and swaps all three for the psi amp's four selector arrows.
// They are exposed over `/v1/ui` as the `readout` list, labeled by meaning, and
// clicking one does exactly what its keyboard action does.
//
// Negative-first: on the base branch `/v1/ui` has no `readout` field and only
// the ammo-cycle arrow exists, so every assertion below fails.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** The readout's controls this frame, by label. */
async function readout(game: GameServer): Promise<UiElement[]> {
  return (await game.ui.state()).readout ?? [];
}

async function button(game: GameServer, label: string): Promise<UiElement> {
  const found = (await readout(game)).find((e) => e.label === label);
  assert.ok(
    found,
    `expected a "${label}" readout control (got ${JSON.stringify(
      (await readout(game)).map((e) => e.label),
    )})`,
  );
  return found;
}

async function labels(game: GameServer): Promise<string[]> {
  return (await readout(game)).map((e) => e.label ?? "");
}

test(
  "the setting button is labeled with the fire mode and opens the settings MFD",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    // Shooter mode has no readout controls at all - the pointer is not up.
    assert.deepEqual(await labels(game), [], "shooter mode exposes no controls");

    // The pistol is the first weapon DebugCycleWeapon hands out.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    // Every gun control the pistol offers, in panel order.
    assert.deepEqual(await labels(game), ["gun_setting", "reload", "cycle_ammo"]);

    const before = (await game.info()).player;
    const setting = await button(game, "gun_setting");
    assert.equal(
      setting.text,
      before.wielded_gun_setting_header,
      "the button is labeled with the mode the gun is actually in",
    );
    assert.equal(setting.text, "NORM");

    // The button OPENS the settings MFD (as the original does) rather than
    // toggling in place - the mode is chosen there, from a described list.
    // `weapon-settings-mfd.e2e.test.ts` covers the panel itself.
    await clickUiElement(game, setting);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).active_panel?.name,
      "Weapon Settings",
      "SETTING docks the weapon settings MFD",
    );
    assert.equal(
      (await game.info()).player.wielded_gun_setting,
      0,
      "opening the panel does not itself switch modes",
    );

    // The direct action (F) still toggles, and the button relabels with it.
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 3 });
    assert.equal((await game.info()).player.wielded_gun_setting_header, "BURST");
    assert.equal((await button(game, "gun_setting")).text, "BURST");
  },
);

test(
  "the reload button reloads the wielded gun",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const pistol = await cycleToWeapon(game, (e) => (e.name ?? "") === "Pistol");
    // Stock the backpack so a reload has something to draw on.
    const clip = (await game.entities.list({ filter: "Standard Clip", limit: 20 })).entities.find(
      (e) => (e.name ?? "") === "Standard Clip",
    );
    assert.ok(clip, "debug_weapons stocks a Standard Clip");
    await game.player.give(clip.id);

    // Spend a couple of rounds so a reload is observable.
    await fireOnce(game);
    await fireOnce(game);
    const spent = ammoOf(await game.entities.detail(pistol.id));

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    await clickUiElement(game, await button(game, "reload"));
    await game.step({ frames: 10 });

    assert.ok(
      ammoOf(await game.entities.detail(pistol.id)) > spent,
      "clicking RELOAD should top the magazine back up",
    );
  },
);

test(
  "an energy weapon keeps its setting button but shows no reload or ammo cycle",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    await cycleToWeapon(game, (e) => (e.name ?? "") === "Laser Pistol");
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    // The laser recharges and fires one projectile type, but it still has an
    // overcharge mode to switch into.
    assert.deepEqual(await labels(game), ["gun_setting"]);
    const setting = await button(game, "gun_setting");
    assert.ok(setting.text, "the laser names its fire mode");
    await clickUiElement(game, setting);
    await game.step({ frames: 3 });
    assert.equal(
      (await game.ui.state()).active_panel?.name,
      "Weapon Settings",
      "the laser's SETTING button opens the same MFD",
    );
  },
);

test(
  "the psi amp swaps the gun controls for its four selector arrows",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // debug_psi auto-equips the amp and unlocks every power.
    await using game = await GameServer.launch({ mission: "debug_psi" });
    await game.step({ frames: 5 });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });

    assert.deepEqual(await labels(game), [
      "psi_tier_prev",
      "psi_tier_next",
      "psi_power_prev",
      "psi_power_next",
    ]);

    const selected = async () => (await game.info()).player.selected_psi_power;
    const start = await selected();
    assert.ok(start, "a power is selected to begin with");

    // A tier step lands somewhere else...
    await clickUiElement(game, await button(game, "psi_tier_next"));
    const tierStep = await selected();
    assert.notEqual(tierStep, start, "the tier arrow changes the selection");

    // ...and a tier step lands on that tier's FIRST power, so from there five
    // more steps walk all five tiers and come back around. (A power step wraps
    // inside one tier instead, so it cannot produce this cycle.)
    const tierBase = tierStep;
    const visited = [];
    for (let i = 0; i < 5; i += 1) {
      await clickUiElement(game, await button(game, "psi_tier_next"));
      visited.push(await selected());
    }
    assert.equal(visited.at(-1), tierBase, "five tier steps wrap through all five tiers");
    assert.equal(new Set(visited).size, 5, `each tier is a distinct stop: ${visited}`);

    // The power arrow moves within the tier: a different power, and stepping
    // back returns to where it started.
    await clickUiElement(game, await button(game, "psi_power_next"));
    const powerStep = await selected();
    assert.notEqual(powerStep, tierBase, "the power arrow changes the selection");
    assert.notEqual(
      powerStep,
      visited[0],
      "stepping the power is not the same as stepping the tier",
    );
    await clickUiElement(game, await button(game, "psi_power_prev"));
    assert.equal(await selected(), tierBase, "and back again");
  },
);
