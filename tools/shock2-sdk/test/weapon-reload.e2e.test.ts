import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end regression test for weapon reload (InputAction::Reload): firing
// drains the clip, and a Reload refills it to the magazine capacity
// (PropBaseGunDesc.clip). Uses the debug_weapons scene + the pistol (12 rounds).
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

test(
  "reload refills the wielded clip to capacity",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8097),
    });

    // debug_weapons starts unarmed; CycleWeapon spawns + wields the pistol.
    await game.step({ frames: 5 });
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 5 });

    const pistolId = (await game.entities.list({ limit: 60 })).entities.find(
      (e) => e.name === "Pistol",
    )?.id;
    assert.ok(pistolId !== undefined, "pistol should be wielded");

    const clip = ammoOf(await game.entities.detail(pistolId));
    assert.ok(clip > 1, `pistol should start with a full clip (got ${clip})`);

    // Drain a few rounds.
    const shots = 4;
    for (let i = 0; i < shots; i++) await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(pistolId)),
      clip - shots,
      "firing consumes one round per shot",
    );

    // Reload: the clip refills to capacity.
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal(
      ammoOf(await game.entities.detail(pistolId)),
      clip,
      "reload refills the clip to capacity",
    );

    // Drain to empty, then a reload restores a full clip (so an empty weapon is
    // usable again - the core point of reload).
    for (let i = 0; i < clip; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "clip should be empty");
    await game.input.trigger("Reload");
    await game.step({ frames: 2 });
    assert.equal(
      ammoOf(await game.entities.detail(pistolId)),
      clip,
      "reloading an empty clip restores a full clip",
    );
  },
);
