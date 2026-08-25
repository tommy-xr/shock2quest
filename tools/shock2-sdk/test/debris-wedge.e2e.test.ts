import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { PhysicsBodySummary } from "../src/types.js";

// Regression coverage for #803 and #815. Smashing an annelid Floor Pod
// flinderizes it into `Eggbit` gibs. `Eggbit` inherits `P$PhysType { SPHERE }`
// and a 30-second TweqDelete from `Monster Parts` (-2182), but no `P$PhysDims`.
// The #597 model-bounds fallback supplies a bounded box, while the explicit
// Flinderize creation mode makes that box dynamic so its authored impulse and
// gravity launch, bounce, settle, and sleep it. The #810 character filter stays
// intact so debris cannot wedge the player or a pursuing creature.
//
// Negative-first for #815: on current main every Eggbit body was kinematic;
// its position was identical after 300 frames and it never slept.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable mission-object id (`template_id`), not a runtime entity id: the Floor
// Pod in the hydro2 cold-storage aisle at (40.0, -8.0, -29.8).
const COLD_STORAGE_POD = 1354;

function gibBodies(bodies: PhysicsBodySummary[]): PhysicsBodySummary[] {
  return bodies.filter((b) => b.entity_name === "Eggbit");
}

function distance(a: PhysicsBodySummary, b: PhysicsBodySummary): number {
  return Math.hypot(
    a.position[0] - b.position[0],
    a.position[1] - b.position[1],
    a.position[2] - b.position[2],
  );
}

function speed(body: PhysicsBodySummary): number {
  return Math.hypot(...body.velocity);
}

test(
  "hydro2: Floor Pod gibs launch, settle, expire, and stay non-solid to characters",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 5 });

    const [pod] = await game.entities.byTemplate(COLD_STORAGE_POD);
    assert.ok(pod, "hydro2 should contain its authored cold-storage Floor Pod");

    const before = gibBodies((await game.physics.bodies()).bodies);
    assert.equal(before.length, 0, "no gibs should exist before the pod breaks");

    // An egg has 10 HP; one hit slays it and fires its Flinderize link.
    await game.entities.sendMessage(pod.id, { type: "Damage", amount: 50 });

    const launched = gibBodies((await game.physics.bodies()).bodies);
    assert.ok(
      launched.length > 0,
      "smashing the pod should flinderize it into Eggbit gibs",
    );

    for (const gib of launched) {
      assert.equal(
        gib.blocks_player,
        false,
        `gib ${gib.body_id} at ${JSON.stringify(gib.position)} must not be solid to the player`,
      );
      assert.equal(
        gib.blocks_actor,
        false,
        `gib ${gib.body_id} at ${JSON.stringify(gib.position)} must not be solid to living actors`,
      );
      // Characters are taken out of the collider's filter, but the gib keeps
      // its body and `entity` membership: it remains raycastable, shootable,
      // and solid to physical projectiles and ordinary movable props.
      assert.equal(gib.body_type, "dynamic");
      assert.equal(gib.is_sensor, false);
      assert.equal(gib.is_sleeping, false);
      assert.ok(
        gib.mass !== null && gib.mass > 0.001 && gib.mass < 1,
        `gib ${gib.body_id} should have bounded model-box mass, got ${gib.mass}`,
      );
      assert.ok(
        gib.collision_groups.includes("entity"),
        `gib ${gib.body_id} must stay raycastable, got ${JSON.stringify(gib.collision_groups)}`,
      );
    }
    assert.ok(
      Math.max(...launched.map(speed)) > 1,
      "Flinderize should immediately apply its authored impulse",
    );

    await game.step({ frames: 30 });
    const moving = gibBodies((await game.physics.bodies()).bodies);
    assert.equal(moving.length, launched.length);
    const launchedByEntity = new Map(launched.map((gib) => [gib.entity_id, gib]));
    const farthestTravel = Math.max(
      ...moving.map((gib) => {
        const start = launchedByEntity.get(gib.entity_id);
        assert.ok(start, `gib ${gib.entity_id} should retain its dynamic body`);
        return distance(start, gib);
      }),
    );
    assert.ok(
      farthestTravel > 0.5,
      `the launched gibs should fly away from their spawn points, max travel ${farthestTravel.toFixed(3)}`,
    );

    // The small bodies must stop consuming solver time once they have bounced
    // and come to rest. Rapier wakes them again normally on a later contact.
    await game.step({ frames: 270 });
    const settled = gibBodies((await game.physics.bodies()).bodies);
    assert.equal(settled.length, launched.length);
    for (const gib of settled) {
      assert.equal(gib.body_type, "dynamic");
      assert.equal(gib.is_sleeping, true, `gib ${gib.body_id} should settle and sleep`);
      assert.ok(speed(gib) < 0.001, `gib ${gib.body_id} should be still after settling`);
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

    // Monster Parts authors a 30-second TweqDelete. Dynamic bodies must still
    // receive normal entity teardown rather than accumulating permanently.
    await game.step({ frames: 1501 });
    const deleted = gibBodies((await game.physics.bodies()).bodies);
    assert.equal(deleted.length, 0, "TweqDelete should remove every gib body");
    const remainingEntities = await game.entities.list({
      filter: "Eggbit",
      limit: 5000,
    });
    assert.equal(
      remainingEntities.entities.filter((entity) => entity.name === "Eggbit").length,
      0,
      "TweqDelete should remove every gib entity",
    );
  },
);
