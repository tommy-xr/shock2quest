import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// End-to-end test against a real debug runtime. Requires game assets in
// Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// medsci1 "Explode Barrel" (template -498, 2 HP) has Corpse -> Incendiary
// Explosion, whose arSrcDesc stim sources are intensity 15 over a 10ft (4
// world-unit) radius. Slaying a barrel must therefore damage entities and
// push dynamic bodies within that radius - not just play the fireball.
const BARREL_NAME = "Explode Barrel";
// A neighbor must sit well inside the 4-unit blast radius so falloff damage
// still exceeds the barrel's 2 HP.
const CHAIN_DISTANCE = 3.0;

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

function speed(v: Vec3): number {
  return Math.hypot(v[0], v[1], v[2]);
}

async function listBarrels(game: GameServer): Promise<EntitySummary[]> {
  const { entities } = await game.entities.list({
    filter: BARREL_NAME,
    limit: 500,
  });
  return entities.filter((e) => e.name === BARREL_NAME);
}

test(
  "Explosion: a slain barrel chain-damages and pushes its surroundings",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8114),
    });

    await game.step({ frames: 2 });

    // Find two barrels within chain range of each other. Entity ids are
    // per-session, so sort by position for a deterministic pick.
    const barrels = (await listBarrels(game)).sort(
      (a, b) => a.position[0] - b.position[0] || a.position[2] - b.position[2],
    );
    assert.ok(barrels.length >= 2, `expected >=2 barrels, got ${barrels.length}`);
    let pair: [EntitySummary, EntitySummary] | undefined;
    outer: for (const a of barrels) {
      for (const b of barrels) {
        if (a.id !== b.id && dist(a.position, b.position) < CHAIN_DISTANCE) {
          pair = [a, b];
          break outer;
        }
      }
    }
    assert.ok(
      pair !== undefined,
      `expected two barrels within ${CHAIN_DISTANCE} units of each other`,
    );
    const [barrelA, barrelB] = pair;

    // Plant a loose pistol next to barrel A. SpawnDebugItem auto-wields; the
    // second spawn drops the first pistol into the world ~4 units along the
    // camera facing. Stand 4 units +X of the barrel facing -X (head.look yaw
    // 0 = -X) so the drop lands at the barrel.
    const [bx, by, bz] = barrelA.position;
    await game.player.teleport({ x: bx + 4.0, y: by + 0.5, z: bz });
    await game.input.set("head.look", [0, 0]);
    await game.step({ frames: 2 });
    for (let i = 0; i < 2; i++) {
      await game.input.trigger("SpawnDebugItem");
      await game.step({ frames: 5 });
    }

    const findLoosePistols = async () => {
      const { bodies } = await game.physics.bodies();
      return bodies.filter(
        (b) =>
          b.entity_name?.includes("Pistol") &&
          b.body_type === "dynamic" &&
          dist(b.position, barrelA.position) < 4.0,
      );
    };
    // Let the dropped pistol fall, bounce, and come to rest (a few seconds).
    let pistolsBefore = await findLoosePistols();
    for (let i = 0; i < 15; i++) {
      await game.step({ frames: 30 });
      pistolsBefore = await findLoosePistols();
      if (
        pistolsBefore.length >= 1 &&
        pistolsBefore.every((b) => speed(b.velocity) < 0.05)
      ) {
        break;
      }
    }
    assert.ok(
      pistolsBefore.length >= 1,
      "expected a loose pistol body near the barrel before the blast",
    );
    const pistolId = pistolsBefore[0].body_id;
    assert.ok(
      speed(pistolsBefore[0].velocity) < 0.05,
      `pistol did not settle before the blast (speed ${speed(
        pistolsBefore[0].velocity,
      ).toFixed(3)} after ~7s)`,
    );

    // Move the player well clear of the blast, then set off barrel A.
    await game.player.teleport({ x: bx + 9.0, y: by + 0.5, z: bz });
    await game.step({ frames: 2 });
    await game.entities.sendMessage(barrelA.id, { type: "Damage", amount: 5.0 });
    // Slay + corpse-spawn happen on the next frame; the explosion's radius
    // blast fires on the frame after that.
    await game.step({ frames: 3 });

    // Push: the pistol beside the barrel must be moving.
    const { bodies: after } = await game.physics.bodies();
    const pistolAfter = after.find((b) => b.body_id === pistolId);
    assert.ok(pistolAfter !== undefined, "pistol body should still exist");
    assert.ok(
      speed(pistolAfter.velocity) > 0.5,
      `expected the blast to push the pistol (speed > 0.5), got ${speed(
        pistolAfter.velocity,
      ).toFixed(3)}`,
    );

    // Damage: barrel B sat inside the blast radius with 2 HP; the blast must
    // chain-slay it. Give the chain a few frames to resolve.
    await game.step({ frames: 5 });
    const remaining = await listBarrels(game);
    const ids = new Set(remaining.map((e) => e.id));
    assert.ok(!ids.has(barrelA.id), "barrel A should be destroyed");
    assert.ok(
      !ids.has(barrelB.id),
      "expected the blast to chain-destroy barrel B inside its radius",
    );
  },
);
