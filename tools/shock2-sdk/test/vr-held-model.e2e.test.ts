import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression tests for held-weapon models (#352): the first-person
// hand models (_h meshes) have their never-visible faces stripped for the
// fixed flat camera, so in VR a held weapon keeps its world model, while flat
// still swaps to the viewmodel for the first-person weapon path.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function modelOf(detail: { properties: { name: string; value: string }[] }): string {
  const p = detail.properties.find((x) => x.name === "Model");
  assert.ok(p, "entity should expose a Model property");
  return p.value;
}

test(
  "VR: a grabbed weapon keeps its world model",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8102),
      debugFlags: ["--vr"],
    });

    // CycleWeapon spawns the pistol; VR wield is a no-op so it drops to the
    // floor in front of the player.
    await game.step({ frames: 10 });
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 90 });

    const pistol = (await game.entities.list({ limit: 100 })).entities.find(
      (e) => e.name === "Pistol",
    );
    assert.ok(pistol, "pistol should have spawned");
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_w");

    // Grab it: park the right hand on the pistol's forward raycast axis
    // (debug_weapons spawns the pawn at the origin with identity rotation, so
    // pawn-local == world minus the pawn position) and squeeze.
    const pawnY = (await game.info()).player.position[1];
    const [px, py, pz] = pistol.position;
    await game.input.set("right_hand.position", [px + 0.4, py - pawnY, pz]);
    await game.input.set("right_hand.squeeze", 1.0);
    await game.step({ frames: 10 });

    const held = (await game.info()).player.right_hand_entity_id;
    assert.equal(held, pistol.id, "pistol should be grabbed by the right hand");

    // The held pistol keeps the world model - no _h viewmodel swap in VR.
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_w");
  },
);

test(
  "flat: a wielded weapon swaps to its first-person viewmodel",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8103),
    });

    // In flat, CycleWeapon spawns AND wields (sends Hold), which swaps the
    // model to the atek_h viewmodel for the first-person weapon path.
    await game.step({ frames: 5 });
    await game.input.trigger("CycleWeapon");
    await game.step({ frames: 10 });

    const pistol = (await game.entities.list({ limit: 100 })).entities.find(
      (e) => e.name === "Pistol",
    );
    assert.ok(pistol, "pistol should be wielded");
    assert.equal(modelOf(await game.entities.detail(pistol.id)), "atek_h");
  },
);
