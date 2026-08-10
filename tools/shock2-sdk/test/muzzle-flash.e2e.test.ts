import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";

// End-to-end regression test for "muzzle flash follows the weapon" (the generic
// RuntimePropAttachment mechanism). In the `debug_weapons` scene we wield the
// pistol, fire (spawning the flash), then TURN the camera while the flash is
// still alive. The flash must track the first-person viewmodel - its offset from
// the weapon stays constant - instead of staying pinned at its spawn pose.
//
// This is the headless counterpart to the Rust unit tests in
// `shock2vr/src/systems/attachment.rs` (which cover the system in isolation).
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

function posOf(entities: EntitySummary[], name: string): Vec3 | undefined {
  return entities.find((e) => e.name === name)?.position;
}

test(
  "muzzle flash tracks the weapon when the camera turns",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_weapons",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8094),
    });

    // debug_weapons starts unarmed; DebugCycleWeapon spawns and wields the pistol.
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 5 });

    // The wielded-entity field (new /v1/info player data) should report the pistol.
    const before = (await game.entities.list({ limit: 60 })).entities;
    const pistolId = before.find((e) => e.name === "Pistol")?.id;
    assert.ok(pistolId !== undefined, "the pistol should exist after DebugCycleWeapon");
    const info = await game.info();
    assert.equal(
      info.player.wielded_entity_id,
      pistolId,
      "the pistol should be reported as wielded",
    );

    // Face the wall (yaw 0) and fire: TriggerPull spawns the muzzle flash.
    await game.input.set("head.look", [0, 0]);
    await game.input.set("right_hand.trigger", 1.0);
    await game.step({ frames: 1 });

    const listA = (await game.entities.list({ limit: 60 })).entities;
    const pistolA = posOf(listA, "Pistol");
    const flashA = posOf(listA, "Assault Flash");
    assert.ok(pistolA, "pistol present after firing");
    assert.ok(flashA, "muzzle flash should spawn on fire");
    const offsetA = dist(pistolA, flashA);

    // Turn the camera 30deg while the flash is still alive (it lives a couple of
    // frames). The viewmodel re-places against the new look direction.
    await game.input.set("head.look", [30, 0]);
    await game.step({ frames: 1 });

    const listB = (await game.entities.list({ limit: 60 })).entities;
    const pistolB = posOf(listB, "Pistol");
    const flashB = posOf(listB, "Assault Flash");
    assert.ok(pistolB, "pistol present after turning");
    assert.ok(flashB, "muzzle flash should still be alive after the turn");

    const offsetB = dist(pistolB, flashB);

    // The viewmodel actually moved with the camera... (the pistol's authored
    // model_offset keeps it ~0.5 world units from the eye, so a 30deg turn
    // moves it ~0.25)
    assert.ok(
      dist(pistolA, pistolB) > 0.15,
      `pistol should move when the camera turns (moved ${dist(pistolA, pistolB).toFixed(3)})`,
    );
    // ...the flash moved too (it isn't pinned at the spawn pose)...
    assert.ok(
      dist(flashA, flashB) > 0.15,
      `flash should move with the weapon, not stay pinned (moved ${dist(flashA, flashB).toFixed(3)})`,
    );
    // ...and crucially its rigid offset from the weapon is preserved.
    assert.ok(
      Math.abs(offsetA - offsetB) < 0.02,
      `flash should keep a constant offset from the weapon (before=${offsetA.toFixed(3)}, after=${offsetB.toFixed(3)})`,
    );
  },
);
