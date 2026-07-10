import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for slow-projectile impact spangs (issue #445): projectiles
// with an initial velocity <= 80 keep a physics body (no raycast script), so
// their impacts arrive as physics `Collided` messages. That path must spawn
// the projectile's authored spang (HitSpang by victim class, MissSpang
// fallback) just like the fast raycast path does - laser bolts, fusion orbs
// and grenades all carry authored spang links that were previously dead data.
//
// Fires the laser pistol (bolt velocity 40 -> physics path) at the
// `debug_weapons` backstop wall and asserts the bolt's authored `MissSpang`
// ("Laser Spang") appears at the wall and then expires with its particles.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "slow projectile impacts spawn the authored spang (laser bolt -> Laser Spang)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8123),
    });

    // Cycle to the laser pistol (4th roster entry).
    await game.step({ frames: 5 });
    for (let i = 0; i < 4; i++) {
      await game.input.trigger("CycleWeapon");
      await game.step({ frames: 3 });
    }
    await game.step({ frames: 5 });

    // Fire at the wall straight ahead (the scene's backstop at x = -12).
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);

    // The bolt flies ~0.5s to the wall and the spang burst is short-lived, so
    // poll in small steps to observe it inside its particle lifetime.
    let spang;
    for (let i = 0; i < 12 && !spang; i++) {
      await game.step({ frames: 10 });
      const spangs = (await game.entities.list({ filter: "Spang", limit: 50 }))
        .entities;
      spang = spangs.find((e) => e.name === "Laser Spang");
    }
    assert.ok(
      spang,
      "a laser bolt wall hit should spawn its authored MissSpang (Laser Spang)",
    );
    // ...at the impact point (the wall face is at x = -11.5; the muzzle is
    // near x = -1), not wherever the bolt spawned.
    assert.ok(
      spang.position[0] < -10,
      `the spang should spawn at the wall (got ${JSON.stringify(spang.position)})`,
    );

    // Spangs are one-shot bursts: they expire with their particles instead of
    // accumulating at every bolt hole.
    await game.step({ frames: 180 }); // 3s >> the particle lifetime
    const after = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    assert.ok(
      !after.some((e) => e.name === "Laser Spang"),
      `the spang should expire after its burst (still present: ${after
        .map((e) => e.name)
        .join(", ")})`,
    );
  },
);
