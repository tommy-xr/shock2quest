import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for attached particle riders: projectiles whose visuals are
// particle-group archetypes linked to them via `ParticleAttachement` (laser
// bolts, psi shots, the fusion orb) get those groups instantiated attached at
// spawn, riding the projectile, and destroyed with it.
//
// Fires the laser pistol in `debug_weapons` and asserts the "Blue Laser Trail"
// particle entity exists while the bolt is in flight and is gone after impact.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "projectile particle riders spawn attached and die with the projectile",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8101),
    });

    // Cycle to the laser pistol (4th roster entry).
    await game.step({ frames: 5 });
    for (let i = 0; i < 4; i++) {
      await game.input.trigger("DebugCycleWeapon");
      await game.step({ frames: 3 });
    }

    // Fire at the wall ahead (the scene's backstop - firing away from it sends
    // the bolt into the void where it never impacts). Check the rider one
    // frame into flight, before the bolt reaches the wall.
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 1 });

    const inFlight = (await game.entities.list({ limit: 100 })).entities;
    const shot = inFlight.find((e) => e.name === "Laser Shot");
    const trail = inFlight.find((e) => e.name === "Blue Laser Trail");
    assert.ok(shot, "the laser bolt should be in flight");
    assert.ok(trail, "the bolt's authored particle rider should be instantiated");
    const dist = Math.hypot(
      trail.position[0] - shot.position[0],
      trail.position[1] - shot.position[1],
      trail.position[2] - shot.position[2],
    );
    assert.ok(dist < 0.5, `the trail should ride the bolt (distance ${dist.toFixed(2)})`);

    await game.input.set("right_hand.trigger", 0.0);

    // After impact the bolt is destroyed - the trail must die with it, not
    // linger at its last transform.
    await game.step({ frames: 120 });
    const after = (await game.entities.list({ limit: 100 })).entities;
    assert.ok(
      !after.some((e) => e.name === "Blue Laser Trail"),
      "the trail should be destroyed with the projectile",
    );
  },
);
