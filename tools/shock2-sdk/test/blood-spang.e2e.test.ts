import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for data-driven impact spangs: a projectile spawns the spang
// its authored links say, not a hardcoded effect.
//
// - Terrain hit: the projectile's `MissSpang` link (pistol bullets -> the
//   "Standard Terr Spang" particle effect).
// - Creature hit: the `HitSpang` link whose victim archetype class the victim
//   descends from (pistol bullets + a hybrid -> "Standard Blood Spang").
//
// Targets medsci1's stationary corridor OG-Pipe hybrid (near [-21.4, -4.7,
// 14.9] - found by name + anchor since entity ids shuffle per launch), and
// aims by computing head.look from the live player/victim positions. Relies
// on the debug runtime's deterministic stepping (paused = fully frozen, so
// positions are stable regardless of HTTP timing).
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const PLAYER_EYE_HEIGHT_WORLD = 1.6; // 4 SS2 units / SCALE_FACTOR

test(
  "projectile impacts spawn the authored spangs (terrain + blood)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8100),
    });

    // Wield the pistol (first CycleWeapon roster entry).
    await game.step({ frames: 5 });
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 5 });

    // Shot 1 - terrain: from spawn, the default facing looks at a wall.
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

    const bloodBefore = afterTerrain.filter((e) => e.name === "Standard Blood Spang").length;

    // Shot 2 - creature. Find the stationary corridor OG-Pipe by name +
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

    // Stand ~4.5 units south (+z) of it, then aim at its chest from the live
    // positions. Empirical debug-runtime look mapping: yaw 0 faces -x, yaw 90
    // faces -z (yaw = atan2(-dz, -dx)); positive pitch aims down.
    await game.player.teleport({
      x: og.position[0],
      y: og.position[1] + 0.8,
      z: og.position[2] + 4.5,
    });
    await game.step({ frames: 15 }); // settle on the floor

    const player = (await game.info()).player;
    const target = (await game.entities.detail(og.id)).position;
    const eye = [
      player.position[0],
      player.position[1] + PLAYER_EYE_HEIGHT_WORLD,
      player.position[2],
    ];
    const d = [
      target[0] - eye[0],
      target[1] + 0.7 - eye[1], // chest height above the entity origin
      target[2] - eye[2],
    ];
    const yawDeg = (Math.atan2(-d[2], -d[0]) * 180) / Math.PI;
    const pitchDeg =
      (Math.atan2(-d[1], Math.hypot(d[0], d[2])) * 180) / Math.PI;

    await game.input.set("head.look", [yawDeg, pitchDeg]);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    const afterBlood = (await game.entities.list({ filter: "Spang", limit: 50 }))
      .entities;
    const bloodSpangs = afterBlood.filter((e) => e.name === "Standard Blood Spang");
    assert.ok(
      bloodSpangs.length > bloodBefore,
      `a hybrid hit should spawn a NEW blood spang (got: ${afterBlood
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
