import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Regression test for #431: re-entering a previously visited level rebuilds it
// from the JSON snapshot taken when leaving. earth.mis object 189 ("Grate
// 6x8") ships a P$Scale with an infinite component; serde_json stores that as
// null, and deserializing it used to panic the game-loop thread with
// "invalid type: null, expected f32". A fresh earth load was always fine -
// only the revisit path crashed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "revisiting earth rebuilds it from save data without crashing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8118),
    });

    await game.step({ frames: 2 });
    assert.equal((await game.info()).mission, "earth.mis");

    // Leaving earth serializes its world into the in-memory save snapshot.
    const away = await game.transitionLevel("station");
    assert.equal(away.mission, "station.mis");

    // Coming back rebuilds earth from that snapshot - the crashing path.
    const back = await game.transitionLevel("earth");
    assert.equal(back.mission, "earth.mis");

    // The runtime is still live: step it and confirm a sane player state.
    await game.step({ frames: 30 });
    const pos = await game.player.position();
    assert.ok(
      Number.isFinite(pos.x) && Number.isFinite(pos.y) && Number.isFinite(pos.z),
      `player should have a finite position back on earth, got ${JSON.stringify(pos)}`,
    );
  },
);
