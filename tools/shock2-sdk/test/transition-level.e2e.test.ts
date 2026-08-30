import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the level-transition (warp) endpoint. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before POST /v1/control/transition-level existed, warping
// levels from HTTP was impossible - a tester could only reach a transition by
// physically entering an in-game trigger volume. This asserts a direct warp
// swaps the active mission and leaves the player live in the new level.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Transitions are deferred behind the loading screen, so the swap only lands
// once the caller pumps frames. Step until the new mission is active.
async function stepUntilMission(
  game: GameServer,
  mission: string,
): Promise<void> {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    if ((await game.info()).mission === mission) {
      return;
    }
    await game.step({ frames: 10 });
  }
  assert.fail(
    `transition to ${mission} never completed (stuck on ${(await game.info()).mission})`,
  );
}

test(
  "transition-level warps between missions and keeps the player live",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });

    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).mission,
      "medsci1.mis",
      "should start in medsci1",
    );

    // Warp to another deck. The switch is deferred, so the immediate result
    // reports the loading screen and the swap lands as frames are pumped.
    await game.transitionLevel("eng1");
    await stepUntilMission(game, "eng1.mis");

    // The new level is live: step it and confirm the player has a finite
    // position (a broken transition would leave no player / a crashed runtime).
    await game.step({ frames: 30 });
    const pos = await game.player.position();
    assert.ok(
      Number.isFinite(pos.x) && Number.isFinite(pos.y) && Number.isFinite(pos.z),
      `player should have a finite position in eng1, got ${JSON.stringify(pos)}`,
    );

    // A second consecutive warp (with an explicit spawn marker) also works -
    // guards against the scene handle going stale after the first switch.
    await game.transitionLevel("hydro1.mis", 1);
    await stepUntilMission(game, "hydro1.mis");

    // A bad warp must be rejected WITHOUT crashing the game-loop thread (the
    // load path does File::open(..).unwrap(), so an unvalidated bad name would
    // brick the runtime for every later command). Assert it errors and the
    // runtime is still live and unchanged afterward.
    await assert.rejects(
      game.transitionLevel("this_level_does_not_exist"),
      /not found|status 404/,
      "warp to a nonexistent level should error, not crash",
    );
    assert.equal(
      (await game.info()).mission,
      "hydro1.mis",
      "runtime should stay live and on hydro1 after a rejected warp",
    );
  },
);
