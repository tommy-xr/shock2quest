import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary } from "../src/types.js";

// Regression coverage for #803. Smashing an annelid Floor Pod flinderizes it
// into `Eggbit` gibs. `Eggbit` inherits `P$PhysType { SPHERE }` from
// `Monster Parts` (-2182) but no `P$PhysDims`, so each gib gets the #597
// model-bounds fallback: an immovable kinematic box, hanging wherever the
// flinder spawned. Dark's SPHERE model is its *simulated, movable* one - the
// player shoves debris aside - so an immovable stand-in is the opposite of
// faithful, and because the character controller has no depenetration pass, a
// gib resting against the capsule resolves every direction to a zero-length
// move. In hydro2's cold storage, where the pods block aisles the player MUST
// smash through, that is a softlock.
//
// Negative-first: on main every spawned gib body reports blocks_player: true.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable mission-object id (`template_id`), not a runtime entity id: the Floor
// Pod in the hydro2 cold-storage aisle at (40.0, -8.0, -29.8).
const COLD_STORAGE_POD = 1354;

function gibBodies(bodies: PhysicsBodySummary[]): PhysicsBodySummary[] {
  return bodies.filter((b) => b.entity_name === "Eggbit");
}

test(
  "hydro2: gibs from a smashed Floor Pod are never solid to the player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8213),
    });
    await game.step({ frames: 5 });

    const [pod] = await game.entities.byTemplate(COLD_STORAGE_POD);
    assert.ok(pod, "hydro2 should contain its authored cold-storage Floor Pod");

    const before = gibBodies((await game.physics.bodies()).bodies);
    assert.equal(before.length, 0, "no gibs should exist before the pod breaks");

    // An egg has 10 HP; one hit slays it and fires its Flinderize link.
    await game.entities.sendMessage(pod.id, { type: "Damage", amount: 50 });
    await game.step({ frames: 30 });

    const gibs = gibBodies((await game.physics.bodies()).bodies);
    assert.ok(
      gibs.length > 0,
      "smashing the pod should flinderize it into Eggbit gibs",
    );

    for (const gib of gibs) {
      assert.equal(
        gib.blocks_player,
        false,
        `gib ${gib.body_id} at ${JSON.stringify(gib.position)} must not be solid to the player`,
      );
      // Only the player is taken out of the collider's filter: the gib keeps
      // its body and its `entity` membership, so it is still raycastable,
      // shootable and solid to everything else.
      assert.equal(gib.body_type, "kinematic");
      assert.equal(gib.is_sensor, false);
      assert.ok(
        gib.collision_groups.includes("entity"),
        `gib ${gib.body_id} must stay raycastable, got ${JSON.stringify(gib.collision_groups)}`,
      );
    }

    // Nothing else about the smash changed: the pod itself is gone and the
    // aisle it blocked is open. Walk the aisle straight through where the pod
    // stood and require the player to get past it.
    const [x, y, z] = pod.position;
    await game.player.teleport({ x: x - 2.5, y: y + 0.5, z });
    await game.step({ frames: 20 });
    const start = (await game.info()).player.position;
    await game.input.lookAtWorldPoint([x + 4, start[1], z]);
    await game.input.set("right_hand.thumbstick", [0, 1]);
    await game.step({ frames: 120 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const end = (await game.info()).player.position;
    assert.ok(
      end[0] > x,
      `the player must walk past the smashed pod at x ${x}: ${start[0]} -> ${end[0]}`,
    );
  },
);
