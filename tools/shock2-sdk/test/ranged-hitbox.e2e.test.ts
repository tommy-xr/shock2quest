import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// A shot resolves to the limb it crossed. The creature's capsule is a movement
// volume - wider than the creature - so a shot that crossed it without touching
// a limb used to damage the creature anyway, reporting no joint. It now carries
// on to whatever is actually behind it.
//
// The exception is a muzzle pressed into a creature: its limbs can all lie
// behind the ray origin, and a point-blank shot must not become a terrain
// impact past the body.
const LATERAL_OFFSETS = [0, 0.2, 0.35, 0.5, 0.65];

test(
  "every shot that damages a creature names the limb it hit",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 30 });

    await game.player.spawnItem("Pistol");
    await game.player.spawnItem("Small Standard Clip");
    await game.step({ frames: 5 });
    await game.input.trigger("DebugCycleWeapon");
    await game.step({ frames: 20 });

    // Every creature in the scene, so a shot passing through one and hitting
    // the next is still checked.
    const creatures = new Map<number, number>();
    let target;
    for (const entity of (await game.entities.list()).entities) {
      const detail = await game.entities.detail(entity.id);
      const points = (detail.aim_points ?? []).length;
      if (points > 1) {
        creatures.set(detail.entity_id, points);
        target ??= detail;
      }
    }
    assert.ok(target, "expected a creature with hitboxes in debug_melee");

    const [tx, ty, tz] = target.position;
    await game.player.teleport({ x: tx + 3.0, y: ty - 1.0, z: tz });
    await game.step({ frames: 20 });

    const seen: Array<[number, number | null | undefined]> = [];
    for (const dz of LATERAL_OFFSETS) {
      const before = (await game.messages.recent()).messages.length;
      await game.input.lookAtWorldPoint([tx, ty + 0.2, tz + dz]);
      await game.step({ frames: 5 });
      await game.input.set("right_hand.trigger", 1);
      await game.step({ frames: 3 });
      await game.input.set("right_hand.trigger", 0);
      await game.step({ frames: 12 });

      for (const message of (await game.messages.recent()).messages.slice(before)) {
        if (message.payload !== "Damage") continue;
        if (!creatures.has(message.to.entity_id)) continue;
        seen.push([message.to.entity_id, message.impact?.bone]);
      }
    }

    assert.ok(seen.length > 0, "the shots should have damaged a creature");
    // Negative-first: the capsule fallback billed creatures with bone null.
    assert.ok(
      seen.every(([, bone]) => bone !== undefined && bone !== null),
      `every creature hit should name a joint; got ${JSON.stringify(seen)}`,
    );
  },
);
