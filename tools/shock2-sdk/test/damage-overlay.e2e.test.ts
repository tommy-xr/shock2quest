import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The floating damage readouts are the instrument for the hitbox work: they
// answer "which limb did that hit, and for how much" without reading hit points
// before and after. They are dev-param gated, drawn in the world pass, and
// tagged `damage_numbers` in /v1/scene so this can assert on them.
const OVERLAY_SOURCE = "damage_numbers";

async function overlayCount(game: GameServer): Promise<number> {
  return (await game.scene.fromSource(OVERLAY_SOURCE)).length;
}

test(
  "damage numbers appear at the impact point, only while the dev param is on",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 20 });

    const creature = (await game.entities.list()).entities.find((entity) =>
      entity.name.startsWith("Entity_"),
    );
    assert.ok(creature, "expected a damageable creature in debug_melee");
    const [x, y, z] = creature.position;
    const damage = () =>
      game.entities.sendMessage(creature.id, {
        type: "Damage",
        amount: 7,
        point: [x, y + 1.2, z],
        direction: [0, 0, 1],
      });

    // Off by default: damage draws nothing.
    await damage();
    await game.step({ frames: 6 });
    assert.equal(
      await overlayCount(game),
      0,
      "no readout should be drawn while `damage_numbers` is off",
    );

    await game.devParams.set("damage_numbers", 1);
    await damage();
    await game.step({ frames: 6 });
    assert.equal(await overlayCount(game), 1, "the blow should draw one readout");

    // ...and it is short-lived, so a firefight does not paper the level over.
    await game.step({ frames: 150 });
    assert.equal(await overlayCount(game), 0, "the readout should expire");

    // A blow landing on a real hitbox collider is dispatched TWICE - once to
    // the hitbox entity, once forwarded to the creature with the struck joint
    // attached - and both carry the same impact point. It must draw exactly
    // one readout, the labeled one; recording both stacks an unlabeled
    // duplicate underneath every real limb hit.
    //
    // Negative-first: without the proxy skip this is 2.
    const hitBoxBody = (await game.physics.bodies({ limit: 5000 })).bodies.find((body) =>
      body.collision_groups.includes("hitbox"),
    );
    assert.ok(hitBoxBody?.entity_id, "expected the creature to have hitbox proxies");
    await game.entities.sendMessage(hitBoxBody.entity_id, {
      type: "Damage",
      amount: 9,
      point: [x, y + 1.5, z],
      direction: [0, 0, 1],
    });
    await game.step({ frames: 6 });
    assert.equal(
      await overlayCount(game),
      1,
      "a hitbox hit should draw one readout, not one per dispatch",
    );

    // ...and it is the labeled one: the forwarded copy carries the joint.
    const traced = (await game.messages.recent()).messages
      .filter((message) => message.payload === "Damage")
      .at(-1);
    assert.ok(
      traced?.impact?.bone !== undefined && traced?.impact?.bone !== null,
      `the forwarded blow should carry its joint, got ${JSON.stringify(traced?.impact)}`,
    );
  },
);
