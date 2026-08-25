import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for data-driven impact spangs: a projectile spawns the spang
// its authored links say, not a hardcoded effect.
//
// - Creature hit: the `HitSpang` link whose victim archetype class the victim
//   descends from (pistol bullets + a hybrid -> "Standard Blood Spang").
// - Terrain hit: the projectile's `MissSpang` link (pistol bullets -> the
//   "Standard Terr Spang" particle effect).
//
// Targets medsci1's corridor OG-Pipe hybrid (near [-21.4, -4.7, 14.9] - found
// by name + anchor since entity ids shuffle per launch), and aims by computing
// head.look from the live player/victim positions. Relies on the debug
// runtime's deterministic stepping (paused = fully frozen, so positions are
// stable regardless of HTTP timing).
//
// Shot ORDER matters (#442): the creature shot must come first, while the
// hybrid is still calm and stationary. Gunfire raises a noise that alerts
// nearby AIs, and this hybrid hears - a wall shot fired first sends it
// walking toward the noise, and a position-snapshot aim then grazes over its
// head (the miss lands on terrain behind it, spawning a second Terr Spang
// instead of the blood spang). The wall doesn't move, so the terrain shot is
// safe to take second, from back at spawn where the alerted hybrid can't
// reach during the test.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";


test(
  "projectile impacts spawn the authored spangs (terrain + blood)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    // Wield the pistol (first DebugCycleWeapon roster entry).
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });

    const spawn = (await game.info()).player.position;

    // Shot 1 - creature, while it is still calm and stationary (no gunfire
    // noise has been raised yet). Find the corridor OG-Pipe by name +
    // position anchor (entity ids are not stable across launches).
    const anchor = { x: -21.4, y: -4.7, z: 14.9 };
    const ogs = (await game.entities.list({ filter: "OG", limit: 10 })).entities.filter(
      (e) => e.name === "OG-Pipe",
    );
    const og = ogs.find(
      (e) =>
        Math.hypot(
          e.position[0] - anchor.x,
          e.position[1] - anchor.y,
          e.position[2] - anchor.z,
        ) < 3.0,
    );
    assert.ok(og, `expected the corridor OG-Pipe near the anchor (got: ${JSON.stringify(ogs.map((e) => e.position))})`);

    // Stand ~4.5 units south (+z) of it, then aim at its torso.
    await game.player.teleport({
      x: og.position[0],
      y: og.position[1] + 0.8,
      z: og.position[2] + 4.5,
    });
    await game.step({ frames: 15 }); // settle on the floor


    const target = (await game.entities.detail(og.id)).position;
    // Production aim at the live torso proxy (reads the runtime camera
    // height, so it survives changes to the player eye).
    await game.player.aimAt(og.id, { hitbox: "torso" });
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    const afterBlood = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    const bloodSpangs = afterBlood.filter((e) => e.name === "Standard Blood Spang");
    assert.ok(
      bloodSpangs.length > 0,
      `a hybrid hit should spawn a blood spang (got: ${afterBlood
        .map((e) => e.name)
        .join(", ")})`,
    );
    // ...and it spawned at the victim, not somewhere else.
    assert.ok(
      bloodSpangs.some(
        (e) =>
          Math.hypot(
            e.position[0] - target[0],
            e.position[1] - (target[1] + 0.7),
            e.position[2] - target[2],
          ) < 2.0,
      ),
      `the blood spang should spawn near the victim (victim at ${JSON.stringify(target)}, spangs at ${JSON.stringify(bloodSpangs.map((e) => e.position))})`,
    );

    // Shot 2 - terrain, from back at spawn (the shot hybrid is alerted and
    // closing in, but a wall can't walk out of the aim): the default facing
    // (yaw 0) looks at a wall.
    await game.player.teleport({ x: spawn[0], y: spawn[1], z: spawn[2] });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 5 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    const afterTerrain = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    assert.ok(
      afterTerrain.some((e) => e.name === "Standard Terr Spang"),
      `a wall hit should spawn the authored terrain spang (got: ${afterTerrain
        .map((e) => e.name)
        .join(", ")})`,
    );

    // Spangs are one-shot bursts: they expire with their particles instead of
    // accumulating forever at every bullet hole.
    await game.step({ frames: 120 }); // 2s >> the ~0.8s particle lifetime
    const afterExpiry = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    assert.equal(
      afterExpiry.length,
      0,
      `spangs should expire after their burst (still present: ${afterExpiry
        .map((e) => e.name)
        .join(", ")})`,
    );
  },
);
