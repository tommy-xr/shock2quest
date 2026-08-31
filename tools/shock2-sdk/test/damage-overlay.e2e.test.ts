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
  const { objects } = await game.scene.objects();
  return objects.filter((object) => object.source === OVERLAY_SOURCE).length;
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
    const damage = (bone?: number) =>
      game.entities.sendMessage(creature.id, {
        type: "Damage",
        amount: 7,
        point: [x, y + 1.2, z],
        direction: [0, 0, 1],
        bone,
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

    // A hitbox-forwarded hit carries the struck joint, and the readout names
    // it - the whole point of the overlay for the hitbox work. Joint 9 is the
    // humanoid head (`creature_definitions::HUMANOID_HIT_BOXES`).
    await damage(9);
    await game.step({ frames: 6 });
    const traced = (await game.messages.recent()).messages
      .filter((message) => message.payload === "Damage")
      .at(-1);
    assert.equal(traced?.impact?.bone, 9, "the injected blow should carry its joint");
    assert.equal(await overlayCount(game), 1, "the labeled blow should draw its readout");
  },
);
