import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end regression test for weapon ammo (PropGunState): a weapon limited by
// its clip decrements one round per shot and dry-fires (no projectile) at empty.
// Uses the debug_weapons scene: wield the pistol (12 rounds), fire it dry, and
// assert the ammo count and the number of bullet-hit impacts on the wall.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "weapon should expose an Ammo property");
  return Number(p.value);
}

function bulletHits(entities: EntitySummary[]): number {
  return entities.filter((e) => e.name === "Bullet Hit").length;
}

test(
  "weapon ammo decrements per shot and dry-fires at empty",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      // Flat is the debug runtime's default presentation; the flat path drives
      // the first-person fire loop.
    });

    // debug_weapons starts unarmed; DebugCycleWeapon spawns + wields the pistol.
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });

    const pistolId = (await game.entities.list({ limit: 60 })).entities.find(
      (e) => e.name === "Pistol",
    )?.id;
    assert.ok(pistolId !== undefined, "pistol should be wielded");

    const startAmmo = ammoOf(await game.entities.detail(pistolId));
    assert.ok(startAmmo > 0, `pistol should start with rounds (got ${startAmmo})`);

    // Fire one round and confirm the count drops by exactly one.
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(pistolId)),
      startAmmo - 1,
      "one shot consumes one round",
    );

    // Drain the rest of the clip.
    for (let i = 1; i < startAmmo; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "clip should be empty");

    // Baseline impact count at empty (absolute count is timing-sensitive since
    // hit-spangs eventually despawn, so we only assert it does not grow below).
    const hitsAtEmpty = bulletHits((await game.entities.list({ limit: 120 })).entities);

    // Dry-fire: pulling on an empty clip must NOT spawn a projectile.
    for (let i = 0; i < 3; i++) await fireOnce(game);
    assert.equal(ammoOf(await game.entities.detail(pistolId)), 0, "ammo stays at 0");
    assert.ok(
      bulletHits((await game.entities.list({ limit: 120 })).entities) <= hitsAtEmpty,
      "dry-firing an empty clip spawns no new projectiles",
    );
  },
);
