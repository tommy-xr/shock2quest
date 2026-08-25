import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

// Production regression for #951. The shipped player-arm `leftswing` motion is
// 31 frames at 30 fps and authors MF_TRIGGER1 at frame 20. The original SS2
// melee path makes the weapon physical at that flag; flat mode resolves its
// aimed raycast at the same event. At the runtime's fixed 60 Hz step, the hit
// therefore lands on simulation frame 41 after the trigger edge, never frame 1.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const TRAINING_DROID = 593;
const WRENCH = -928;

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find(
    (candidate) => candidate.name === "HitPoints",
  );
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

test(
  "flat melee damages once at the authored swing event, not on trigger",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 5 });

    // Provision and wield the real player Wrench through production inventory
    // selection. Runtime ids shuffle, so discover both objects each launch.
    await game.player.spawnItem(WRENCH);
    await game.input.trigger("EquipWrench");
    await game.step({ frames: 5 });
    const [droid] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(droid, "Earth should contain its authored Training Droid");

    // The droid is stationary on its authored platform. Settle just inside the
    // Wrench ray's 1.2-world-unit reach and aim at a discovered torso proxy.
    const [x, y, z] = (await game.entities.detail(droid.id)).position;
    await game.player.teleport({ x: x + 1.2, y: y + 1, z });
    await game.step({ frames: 60 });
    const aim = await game.player.aimAt(droid, {
      hitbox: "torso",
      visibility: "required",
    });
    assert.equal(aim.entity_id, droid.id);
    await game.step({ frames: 3 });

    const before = hitPoints(await game.entities.detail(droid.id));
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });
    assert.equal(
      hitPoints(await game.entities.detail(droid.id)),
      before,
      "the trigger edge starts the visible swing but must not damage",
    );

    // Release/re-pull chatter while the arm is already swinging must neither
    // restart the clip before its event nor manufacture a second attack.
    await game.input.set("right_hand.trigger", 0);
    await game.step({ frames: 1 });
    await game.input.set("right_hand.trigger", 1);
    await game.step({ frames: 1 });

    // Three frames elapsed above; stop at total frame 40, immediately before
    // frame-20 MF_TRIGGER1 is delivered by the 30-fps motion.
    await game.step({ frames: 37 });
    assert.equal(
      hitPoints(await game.entities.detail(droid.id)),
      before,
      "damage must wait for leftswing's authored impact event",
    );

    await game.step({ frames: 1 });
    const afterImpact = hitPoints(await game.entities.detail(droid.id));
    assert.equal(
      afterImpact,
      before - 6,
      "MF_TRIGGER1 should resolve exactly one aimed Wrench hit",
    );

    await game.step({ frames: 80 });
    assert.equal(
      hitPoints(await game.entities.detail(droid.id)),
      afterImpact,
      "one swing event must not deal repeated damage while trigger stays held",
    );
    await game.input.set("right_hand.trigger", 0);
  },
);
