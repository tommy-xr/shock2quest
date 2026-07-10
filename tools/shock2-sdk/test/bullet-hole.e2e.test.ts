import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the bullet-hole decal's lifetime (#447): the
// "Bullet Hit" sprite rides the "Standard Terr Spang" a wall hit spawns, and
// carries its own authored 10s delete tweq. It must NOT die with the spang
// when the spang's ~0.8s particle burst expires - the remove_entity rider
// cascade detaches self-expiring riders instead of deleting them - and it
// must still be destroyed by its own tweq at the authored time (no leak).
//
// Riders WITHOUT a delete tweq must still die with their host - that side is
// covered by projectile-trail.e2e.test.ts (the laser bolt's trail).
//
// Fires the pistol at a wall from medsci1's spawn (the default facing looks
// at a wall), following blood-spang.e2e.test.ts.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "bullet-hole decal outlives its spang and expires on its own delete tweq",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8123),
    });

    const bulletHoles = async () =>
      (await game.entities.list({ filter: "Bullet Hit", limit: 50 })).entities.filter(
        (e) => e.name === "Bullet Hit",
      );

    // Wield the pistol (first CycleWeapon roster entry).
    await game.step({ frames: 5 });
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 5 });

    // Sanity: no decals before the shot.
    assert.equal(
      (await bulletHoles()).length,
      0,
      "no Bullet Hit entities should exist before the shot",
    );

    // Fire at the wall the default facing (yaw 0) looks at.
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    // (a) The wall hit spawned the terrain spang's authored decal rider.
    const justAfter = await bulletHoles();
    assert.equal(
      justAfter.length,
      1,
      `a wall hit should spawn one Bullet Hit decal (got ${justAfter.length})`,
    );

    // (b) 2s later the spang's one-shot burst has expired and the spang is
    // destroyed - but the decal manages its own lifetime (10s delete tweq)
    // and must survive the host's rider cascade.
    await game.step({ frames: 120 });
    const spangs = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    assert.equal(
      spangs.length,
      0,
      `the spang should expire with its burst (still present: ${spangs
        .map((e) => e.name)
        .join(", ")})`,
    );
    const afterSpang = await bulletHoles();
    assert.equal(
      afterSpang.length,
      1,
      "the Bullet Hit decal should outlive the spang burst (authored 10s lifetime)",
    );

    // (c) ...and past its authored 10s, its own delete tweq destroys it -
    // detached decals must not leak. ~2.2s have elapsed since the shot; run
    // well past the 10s mark.
    await game.step({ frames: 600 }); // +10s => ~12.2s since the shot
    const afterTweq = await bulletHoles();
    assert.equal(
      afterTweq.length,
      0,
      "the Bullet Hit decal should be destroyed by its own delete tweq",
    );
  },
);
