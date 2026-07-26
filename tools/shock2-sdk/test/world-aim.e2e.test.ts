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
    assert.ok(initial.aim_points?.some((point) => point.classification === "torso"));

    await game.input.set("left_hand.thumbstick", [0.75, 0]);
    await game.step({ frames: 20 });
    await game.input.set("left_hand.thumbstick", [0, 0]);
    await game.save("world-aim-e2e");
    await game.load("world-aim-e2e");
    const pawn = (await game.info()).player.rotation;
    assert.ok(Math.abs(pawn[1]) > 0.01 || Math.abs(pawn[3] - 1) > 0.01);

    const aim = await game.player.aimAt(monster, { hitbox: "torso" });
    assert.equal(aim.entity_id, monster.id);
    assert.equal(aim.classification, "torso");
    assert.equal(aim.fallback_used, false);
    await game.step({ frames: 2 });

    await game.input.set("right_hand.trigger_value", 1);
    await game.step({ frames: 3 });
    await game.input.set("right_hand.trigger_value", 0);
    await game.step({ frames: 90 });
    assert.ok(hitPoints(await game.entities.detail(monster.id)) < hitPoints(initial));
  },
);
