import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { describeSounds } from "./helpers/audio.js";
import { ammoOf, cycleToWeapon, fireOnce } from "./helpers/weapon.js";

// End-to-end coverage for fire-mode switching (InputAction::CycleGunSetting).
// SS2 guns have exactly two modes; switching swaps the firing description and
// the ammo set, keeping the chosen ammo TYPE across the switch by its
// ProjectileOptions.order. Observable headlessly via /v1/info's
// wielded_gun_setting / wielded_gun_setting_header / wielded_ammo_type.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function fireMode(game: GameServer): Promise<[number | null, string | null]> {
  const player = (await game.info()).player;
  return [player.wielded_gun_setting, player.wielded_gun_setting_header];
}

test(
  "the pistol switches between its NORM and BURST modes and back",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });

    await game.step({ frames: 5 });
    assert.deepEqual(await fireMode(game), [null, null], "unarmed has no fire mode");

    // The pistol is the first weapon DebugCycleWeapon hands out.
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    assert.deepEqual(await fireMode(game), [0, "NORM"], "pistol starts on its first mode");

    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.deepEqual(await fireMode(game), [1, "BURST"], "switches to the 3-round burst");

    // Retail plays the `bset` sting on a mode change.
    const played = (await game.audio.recent()).sounds.filter((s) => s.sequence > before);
    assert.ok(
      played.some((s) => s.sample.toLowerCase().startsWith("bset")),
      `the switch should play the bset sting, got ${describeSounds(played)}`,
    );

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });
    assert.deepEqual(await fireMode(game), [0, "NORM"], "and back - there are only two modes");
  },
);

test(
  "a shotgun mode switch keeps the selected ammo type",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    const shotgun = await cycleToWeapon(game, (e) => e.name.includes("Shotgun"));
    assert.deepEqual(await fireMode(game), [0, "NORM"], "shotgun starts on its first mode");
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "rslug",
      "the shotgun's first ammo type in order is the rifled slug",
    );

    // Cycling ammo needs an empty magazine (a loaded one is ejected instead),
    // so fire it dry first. Pellets are the SECOND ammo type in order, which is
    // what makes the remap below observable: a switch that reset the selection
    // would land back on slugs.
    const loaded = ammoOf(await game.entities.detail(shotgun.id));
    assert.ok(loaded > 0, "debug shotgun starts loaded");
    for (let i = 0; i < loaded; i++) await fireOnce(game);
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_ammo_type, "pellet", "pellets selected");

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });

    assert.deepEqual(await fireMode(game), [1, "TRIPLE"], "switches to the double load");
    assert.equal(
      (await game.info()).player.wielded_ammo_type,
      "pellet",
      "the double load keeps pellets (Double Pellet shares Pellet's order)",
    );
  },
);

test(
  "a gun with only one fire mode ignores the switch",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons" });
    await game.step({ frames: 5 });

    // The psi amp links no setting-specific ammo and names no second mode.
    await cycleToWeapon(game, (e) => e.name.includes("Psi Amp"));
    const before = await fireMode(game);
    assert.equal(before[0], 0, "psi amp is on setting 0");

    await game.input.trigger("CycleGunSetting");
    await game.step({ frames: 2 });

    assert.deepEqual(await fireMode(game), before, "a one-mode gun does not switch");
  },
);
