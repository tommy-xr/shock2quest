import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

function hitPoints(detail: EntityDetailResult): number {
  return Number(
    detail.properties.find((property) => property.name === "HitPoints")?.value ?? 0,
  );
}

test(
  "built SDK aims at a live torso after rotated save/load and ordinary fire hits",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_psi",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8221),
    });
    await game.step({ frames: 5 });

    const before = new Set(
      (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(
        (entity) => entity.id,
      ),
    );
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const monster = (
      await game.entities.list({ filter: "OG-Pipe", limit: 50 })
    ).entities.find((entity) => !before.has(entity.id));
    assert.ok(monster, "SpawnDebugMonster should create a live target");

    const initial = await game.entities.detail(monster.id);

    await game.input.set("left_hand.thumbstick", [0.75, 0]);
    await game.step({ frames: 20 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.save("world-aim-e2e");
    await game.load("world-aim-e2e");
    const pawn = (await game.info()).player.rotation;
    assert.ok(Math.abs(pawn[1]) > 0.01 || Math.abs(pawn[3] - 1) > 0.01);
    const restoredMonster = (
      await game.entities.list({ filter: "OG-Pipe", limit: 50 })
    ).entities
      .filter((entity) => entity.template_id === monster.template_id)
      .sort(
        (a, b) =>
          Math.hypot(
            a.position[0] - initial.position[0],
            a.position[1] - initial.position[1],
            a.position[2] - initial.position[2],
          ) -
          Math.hypot(
            b.position[0] - initial.position[0],
            b.position[1] - initial.position[1],
            b.position[2] - initial.position[2],
          ),
      )[0];
    assert.ok(restoredMonster, "saved spawned creature should be rediscovered after load");
    const restored = await game.entities.detail(restoredMonster.id);
    const classifications = new Set(
      restored.aim_points?.map((point) => point.classification),
    );
    assert.ok(classifications.has("head"), "restored creature should expose a head proxy");
    assert.ok(
      classifications.has("torso"),
      "restored creature should expose a torso proxy",
    );
    assert.ok(classifications.has("limb"), "restored creature should expose limb proxies");

    const aim = await game.player.aimAt(restoredMonster, { hitbox: "torso" });
    assert.equal(aim.entity_id, restoredMonster.id);
    assert.equal(aim.classification, "torso");
    assert.equal(aim.fallback_used, false);
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
