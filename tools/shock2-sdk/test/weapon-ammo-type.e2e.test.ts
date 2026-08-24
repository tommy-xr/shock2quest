import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end regression test for ammo-type cycling (InputAction::CycleAmmo). The
// pistol carries three Projectile links (std / he / ap); cycling advances the
// selected one and wraps. Observable headlessly via /v1/info.wielded_ammo_type.
// What a cycle does to a LOADED magazine is weapon-ammo-eject's subject.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function ammoType(game: GameServer): Promise<string | null> {
  return (await game.info()).player.wielded_ammo_type;
}

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

test(
  "cycling advances the selected ammo type and wraps",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8099),
    });

    // Unarmed: no ammo type.
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), null, "unarmed has no ammo type");

    // Wield the pistol (std / he / ap).
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });
    assert.equal(await ammoType(game), "std", "pistol starts on its first ammo type");
    const pistolId = (await game.info()).player.wielded_entity_id;
    assert.ok(pistolId !== null, "pistol should be wielded");

    // Cycling a LOADED magazine ejects it to the backpack first, which is
    // weapon-ammo-eject's subject. This test is about the selection ORDER, so
    // fire the magazine off and cycle from empty.
    const loaded = ammoOf(await game.entities.detail(pistolId));
    assert.ok(loaded > 0, "debug pistol starts loaded");
    for (let i = 0; i < loaded; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "pistol is empty");

    // Once empty, cycle through all three and wrap back.
    const sequence: string[] = [];
    for (let i = 0; i < 3; i++) {
      await game.input.trigger("CycleAmmo");
      await game.step({ frames: 2 });
      sequence.push((await ammoType(game)) ?? "<null>");
    }
    // The cycle order follows ProjectileOptions.order (the data's intended
    // order), not the raw link order: std -> ap -> he -> std.
    assert.deepEqual(
      sequence,
      ["ap", "he", "std"],
      "cycling advances in ProjectileOptions.order and wraps back to std",
    );
  },
);
