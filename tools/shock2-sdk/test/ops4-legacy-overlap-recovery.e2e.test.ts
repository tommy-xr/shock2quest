import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Position } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const saveName =
  "campaign_ops_shodan_25th_iter03_replay_ops4_wedge_20260808";
const assetRoot = process.env.DARK_ASSET_PATH;
const saveAvailable =
  assetRoot !== undefined &&
  existsSync(join(assetRoot, "saves", `${saveName}.sav`));
const port = Number(process.env.SHOCK2_OPS4_OVERLAP_E2E_PORT ?? 9484);

const trapped: Position = {
  x: 35.173008,
  y: -8.355836,
  z: -40.280003,
};

function distance(a: Position, b: Position): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z);
}

function horizontalDistance(a: Position, b: Position): number {
  return Math.hypot(a.x - b.x, a.z - b.z);
}

test(
  "legacy Ops4 save recovers a wedged player capsule before locomotion",
  { skip: !e2eEnabled || !saveAvailable, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
      port,
    });

    const loaded = await game.load(saveName);
    assert.equal(loaded.mission, "ops4.mis");
    const restored = await game.player.position();
    assert.ok(
      distance(restored, trapped) < 0.05,
      `fixture must restore the known legacy pose before the first physics step: ${JSON.stringify(restored)}`,
    );

    // Save loading rebuilds Rapier; its first fixed step builds the query tree,
    // audits the restored standing/crouched capsule, and performs the bounded
    // deterministic recovery before ordinary movement is committed.
    await game.step({ frames: 1 });
    const recovered = await game.player.position();
    assert.ok(
      distance(recovered, trapped) > 0.5 && distance(recovered, trapped) < 2.1,
      `overlapping legacy pose should recover nearby, got ${JSON.stringify(recovered)}`,
    );

    // Exercise the production thumbstick controller from the recovered pose.
    // This same three-second heading advanced less than half a unit in the
    // negative fixture and then remained pinned between the pipe/ramp faces.
    await game.input.set("right_hand.thumbstick", [0, -1]);
    await game.step({ frames: 180 });
    await game.input.set("right_hand.thumbstick", [0, 0]);
    const escaped = await game.player.position();
    assert.ok(
      horizontalDistance(escaped, recovered) > 3.0,
      `recovered player should walk out through production locomotion: recovered=${JSON.stringify(recovered)}, escaped=${JSON.stringify(escaped)}`,
    );
  },
);
