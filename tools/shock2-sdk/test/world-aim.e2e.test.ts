import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const TRAINING_DROID = 593;

function hitPoints(detail: EntityDetailResult): number {
  return Number(
    detail.properties.find((property) => property.name === "HitPoints")?.value ?? 0,
  );
}

test(
  "built SDK aims at a live torso after rotated save/load and ordinary fire hits",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Persistent saves reload through the real mission loader, so exercise a
    // mounted mission rather than a synthetic debug_* scene. On the parent
    // revision, saving debug_psi records that scene name and load panics because
    // no corresponding mission asset exists.
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8221),
    });
    await game.step({ frames: 5 });
    // Provision and auto-wield the same loaded pistol used by the debug action.
    await game.input.trigger("SpawnDebugItem");
    await game.step({ frames: 5 });

    const [monster] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(monster, "Earth should contain the live Training Droid");
    const initial = await game.entities.detail(monster.id);
    const [x, y, z] = initial.position;
    // The authored platform four units to the droid's +X is collision-valid
    // and leaves enough clearance for the flat weapon's muzzle.
    await game.player.teleport({ x: x + 4, y, z });
    await game.step({ frames: 30 });

    await game.input.set("left_hand.thumbstick", [0.75, 0]);
    await game.step({ frames: 20 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.save("world-aim-e2e");
    await game.load("world-aim-e2e");
    const pawn = (await game.info()).player.rotation;
    assert.ok(Math.abs(pawn[1]) > 0.01 || Math.abs(pawn[3] - 1) > 0.01);
    const [restoredMonster] = await game.entities.byTemplate(TRAINING_DROID);
    assert.ok(restoredMonster, "saved creature should be rediscovered after load");
    const restored = await game.entities.detail(restoredMonster.id);
    const classifications = new Set(
      restored.aim_points?.map((point) => point.classification),
    );
    assert.ok(
      classifications.has("torso"),
      "restored creature should expose a torso proxy",
    );

    const aim = await game.player.aimAt(restoredMonster, {
      hitbox: "torso",
      visibility: "required",
    });
    assert.equal(aim.entity_id, restoredMonster.id);
    assert.equal(aim.classification, "torso");
    assert.equal(aim.fallback_used, false);
    assert.equal(aim.visibility.state, "visible");
    assert.equal(aim.visibility.origin, "view");
    await game.step({ frames: 2 });

    await game.input.set("right_hand.trigger_value", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger_value", 0);
    await game.step({ frames: 90 });
    assert.ok(
      hitPoints(await game.entities.detail(restoredMonster.id)) < hitPoints(restored),
    );
  },
);
