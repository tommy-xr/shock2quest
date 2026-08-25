import assert from "node:assert/strict";
import { test } from "node:test";

import { AimOcclusionError, GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops2: required head aim reports the Midwife doorway occluder",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
    });
    const midwife = (
      await game.entities.list({ filter: "Midwife", limit: 20 })
    ).entities.find((entity) => entity.template_id === 352);
    assert.ok(midwife, "ops2 should contain the Midwife");

    // Campaign reproduction: just outside the small room at z=83, where the
    // classified head point is behind the doorway frame.
    await game.player.teleport({ x: 62.08, y: -15.796, z: 83 });
    await game.step({ frames: 2 });

    await assert.rejects(
      game.player.aimAt(midwife, {
        hitbox: "head",
        visibility: "required",
      }),
      (error: unknown) => {
        if (!(error instanceof AimOcclusionError)) return false;
        const { result } = error;
        assert.equal(result.classification, "head");
        assert.equal(result.fallback_used, false);
        assert.equal(result.visibility.state, "blocked");
        assert.equal(result.visibility.origin, "view");
        assert.ok(result.visibility.blocker?.hit_point);
        assert.ok(
          result.visibility.blocker?.entity_id !== null ||
            result.visibility.blocker?.body_id !== null,
          "occlusion should identify its entity or rigid body",
        );
        assert.ok(
          (result.visibility.blocker?.distance ?? Number.POSITIVE_INFINITY) <
            result.visibility.target_distance,
        );
        return true;
      },
    );
  },
);
