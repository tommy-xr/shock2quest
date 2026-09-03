import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import {
  collisionSoundsSince,
  describeSounds as describe,
  tagValue,
} from "./helpers/audio.js";
import { waitForShotReady } from "./helpers/weapon.js";

// End-to-end test for weapon impact sounds: a bullet hit plays the
// material-tagged collision schema (event=collision + the projectile's
// ammotype class tag + the hit surface's material), observed via the debug
// runtime's played-sound log (GET /v1/audio/recent) - the only headless way
// to see audio.
//
// - Creature hit: the victim's inherited PropMaterial ("Material FleshTarget"
//   via the Hybrids archetype) -> (event=collision, ammotype=std,
//   material=fleshtarget) -> a bulftar* flesh thud.
// - Terrain hit: a metal-family clang (bulmet*), distinct from the flesh
//   thud. The surface ahead of spawn is an entity carrying PropMaterial
//   "Material MetalBig"; bare world geometry (no per-texture material lookup
//   in the port yet) falls back to the default material, also metal.
//
// Aiming/ordering mirrors blood-spang.e2e.test.ts: target medsci1's corridor
// OG-Pipe hybrid (near [-21.4, -4.7, 14.9], found by name + anchor since
// entity ids shuffle per launch) and shoot it FIRST, while it is still calm
// and stationary - gunfire noise alerts it and a moving target breaks a
// position-snapshot aim. The wall shot is safe to take second from spawn.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";


test(
  "bullet impacts play material-tagged collision schemas (flesh vs terrain)",
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


    const beforeCreature =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;

    const target = (await game.entities.detail(og.id)).position;
    // Production aim at the live torso proxy (reads the runtime camera
    // height, so it survives changes to the player eye).
    await game.player.aimAt(og.id, { hitbox: "torso" });
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    const afterCreature = collisionSoundsSince(
      (await game.audio.recent()).sounds,
      beforeCreature,
    );
    assert.ok(
      afterCreature.length > 0,
      "a creature hit should play a collision-schema impact sound",
    );
    // ...tagged with the victim's material (flesh thud, not a generic clang).
    assert.ok(
      afterCreature.some((s) => tagValue(s, "material") === "fleshtarget"),
      `the creature impact should resolve material=fleshtarget (got: ${describe(afterCreature)})`,
    );
    // ...at the victim, not at the shooter.
    assert.ok(
      afterCreature.some(
        (s) =>
          Math.hypot(
            s.position[0] - target[0],
            s.position[1] - (target[1] + 0.7),
            s.position[2] - target[2],
          ) < 2.0,
      ),
      `the impact sound should play near the victim (victim at ${JSON.stringify(target)}, sounds at ${JSON.stringify(afterCreature.map((s) => s.position))})`,
    );

    // Shot 2 - terrain, from back at spawn (the shot hybrid is alerted and
    // closing in, but a wall can't walk out of the aim): the default facing
    // (yaw 0) looks at a wall.
    await game.player.teleport({ x: spawn[0], y: spawn[1], z: spawn[2] });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 5 });

    // The pistol waits 500 ms between shots; the creature shot above is still
    // being paid for.
    await waitForShotReady(game);

    const beforeTerrain =
      (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;

    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.trigger", 0.0);
    await game.step({ frames: 1 });

    const afterTerrain = collisionSoundsSince(
      (await game.audio.recent()).sounds,
      beforeTerrain,
    );
    assert.ok(
      afterTerrain.length > 0,
      "a wall hit should play a collision-schema impact sound",
    );
    // A metal-family material (metalbig from the surface entity's
    // PropMaterial, or the default metal for bare world geometry) - a clang,
    // distinct from the creature shot's flesh thud.
    assert.ok(
      afterTerrain.some((s) => tagValue(s, "material")?.startsWith("metal")),
      `the terrain impact should resolve a metal-family material (got: ${describe(afterTerrain)})`,
    );
  },
);
