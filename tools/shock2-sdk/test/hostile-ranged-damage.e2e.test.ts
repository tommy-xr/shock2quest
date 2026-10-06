import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "a hostile turret tracks and damages the player",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "debug_turret",
    });

    const [turret] = await game.entities.byTemplate(-168);
    assert.ok(turret, "the debug scene's laser turret");
    const [x, y, z] = (await game.entities.detail(turret.id)).position;
    // Turrets face model -X. This scene mounts it at 180 degrees, so stand
    // on its +X side inside the authored detection cone, above the floor.
    await game.player.teleport({ x: x + 4, y: y + 1, z });
    await game.step({ frames: 30 });

    const hpBefore = (await game.info()).player.hit_points;
    assert.ok(hpBefore !== null, "debug player should expose hit points");

    // The authored turret takes 2.5s to open, creates its ranged-weapon proxy
    // on the first firing tick, then fires once per second. Six seconds gives
    // that real state machine enough time to land multiple shots.
    await game.step({ frames: 360 });

    const hpAfter = (await game.info()).player.hit_points;
    assert.ok(hpAfter !== null, "debug player should expose hit points");
    assert.ok(
      hpAfter < hpBefore,
      `the turret should damage the player after opening (${hpBefore} -> ${hpAfter})`,
    );
  },
);
