import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// medsci1's crawl scene: walking up to `crawlscene` sends a crew woman
// running for `crawlrun` with a shotgun hybrid after her; at the end of the
// chase the hybrid frobs `killcrawl`, which removes her. Both walk through
// closed doors, so the scene only finishes if scripted walks open them.
const TRIGGER = { x: -23.57, y: 0.84, z: -15.44 };
const FINISH_SECONDS = 25;

test(
  "medsci1's crawl scene runs through its doors to the authored ending",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 5 });

    const women = async () =>
      (await game.entities.list({ filter: "FemaleMedsci" })).entities.length;
    const before = await women();
    assert.ok(before > 0, "medsci1 should contain the crawl scene's crew woman");

    await game.player.teleport(TRIGGER);
    let finishedAt: number | null = null;
    for (let second = 1; second <= FINISH_SECONDS; second++) {
      await game.step({ frames: 60 });
      if ((await women()) < before) {
        finishedAt = second;
        break;
      }
    }
    assert.ok(
      finishedAt !== null,
      `the scene should end (killcrawl) within ${FINISH_SECONDS} s instead of stalling at a door`,
    );
  },
);
