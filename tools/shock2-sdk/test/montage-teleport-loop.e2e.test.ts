import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

// End-to-end regression for #515. Requires game assets in Data/ and compiles the
// runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// The bug: earth.mis's "years of service" montage RETURN teleport trap drops the
// player at the vestibule return spot (~11.6, 24.5, 65.79), which sits INSIDE the
// montage ENTRY tripwire's box. Since PR #362 removed teleport-entry suppression
// globally, the scripted return teleport re-fired the entry tripwire -> re-teleport
// to the montage viewing spot (~5.6, 22, 237) -> infinite loop; the player could
// never leave the montage.
//
// The fix suppresses tripwire ENTER only for SCRIPTED teleport-trap arrivals, while
// still firing ENTER for VR/walk locomotion teleports (so the ops1 cutscene tripwire
// keeps working). This test triggers the REAL return teleport trap and asserts the
// player STAYS at the return spot instead of bouncing to the montage.
//
// Negative-first: on pre-fix code the player bounces to z~=237 (assertion fails).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The return teleport trap's own position (it teleports the player to itself).
const RETURN_SPOT = { x: 11.6084385, y: 24.26012, z: 65.788414 };
// Where the entry tripwire teleports the player if it re-fires (the montage).
const MONTAGE_ENTRY_Z = 237.41374;

function dist(a: EntitySummary["position"], b: { x: number; y: number; z: number }): number {
  return Math.hypot(a[0] - b.x, a[1] - b.y, a[2] - b.z);
}

test(
  "earth montage: scripted return teleport does not re-fire the entry tripwire (#515)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8137),
    });
    await game.step({ frames: 5 });

    // Discover the return teleport trap by its (stable) position - runtime entity
    // ids are not stable across runs, so never hardcode them.
    const { entities } = await game.entities.list({ filter: "Player Teleport Trap" });
    const traps = entities.filter((e) => e.name === "Player Teleport Trap");
    assert.ok(traps.length > 0, "earth.mis should have Player Teleport Trap entities");
    const returnTrap = traps
      .slice()
      .sort((a, b) => dist(a.position, RETURN_SPOT) - dist(b.position, RETURN_SPOT))[0];
    assert.ok(
      dist(returnTrap.position, RETURN_SPOT) < 1.0,
      `expected a return teleport trap at ~${JSON.stringify(RETURN_SPOT)}, ` +
        `nearest was ${JSON.stringify(returnTrap.position)}`,
    );

    // Fire the return teleport trap (as the montage sequence does at the end):
    // TrapTeleportPlayer repositions the player to the trap's own spot, inside the
    // entry tripwire's box.
    await game.entities.sendMessage(returnTrap.id, { type: "TurnOn" });
    await game.step({ frames: 10 });

    const pos = await game.player.position();

    // Post-fix: the player stays at the return spot. Pre-fix: the entry tripwire
    // re-fires and bounces the player to the montage viewing spot (z ~= 237).
    assert.ok(
      Math.abs(pos.z - RETURN_SPOT.z) < 3.0,
      `player should stay at the return spot z~=${RETURN_SPOT.z}, got ${JSON.stringify(pos)} ` +
        `(bounced to the montage if z~=${MONTAGE_ENTRY_Z})`,
    );
    assert.ok(
      Math.abs(pos.z - MONTAGE_ENTRY_Z) > 50.0,
      `player must NOT bounce to the montage entry (z~=${MONTAGE_ENTRY_Z}), got ${JSON.stringify(pos)}`,
    );

    // Now WALK OUT of the entry tripwire box. This is the real test of #515: if
    // the scripted arrival was tracked as "present", leaving fires an unbalanced
    // EXIT TurnOff -> the montage trap re-fires (TrapTeleportPlayer teleports on
    // ANY message, including TurnOff) -> the player is yanked back to the montage.
    // The fix ignores the scripted arrival entirely, so walking out is a no-op.
    await game.input.set("right_hand.thumbstick", [0.0, -1.0]);
    await game.step({ frames: 60 });
    await game.input.set("right_hand.thumbstick", [0.0, 0.0]);

    const outPos = await game.player.position();
    assert.ok(
      Math.abs(outPos.z - MONTAGE_ENTRY_Z) > 50.0,
      `walking out of the box must NOT bounce the player to the montage entry ` +
        `(z~=${MONTAGE_ENTRY_Z}), got ${JSON.stringify(outPos)}`,
    );
  },
);
