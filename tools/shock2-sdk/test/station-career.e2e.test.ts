import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { stepPastCutscenes } from "./helpers/cutscenes.js";

// End-to-end test for the station training-year progression. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: ChooseMissionScript::get_current_year returned the LOWEST
// completed training_year_N bit, so once years 2 and 3 were both set it computed
// year 3 forever (< 4), re-looping station.mis and NEVER reaching the year-4
// deploy-to-medsci1 branch - the intro was uncompletable. This fires the
// training trigger repeatedly and asserts it deploys to medsci1 within a few
// tours (it looped indefinitely before the fix).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "station: training rounds advance and deploy to medsci1",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
    });
    await game.step({ frames: 10 });

    // Fire the ChooseMission trigger (the transition whose destination is
    // medsci1); station reloads between tours, so re-find it each time.
    let mission = (await game.info()).mission;
    for (let fire = 0; fire < 6 && mission === "station.mis"; fire++) {
      const trigger = (await game.transitions()).transitions.find((t) =>
        t.dest_level.toLowerCase().includes("medsci1"),
      );
      assert.ok(trigger, "station should expose a ChooseMission trigger to medsci1");
      // TurnOn is a valid debug message; the SDK's typed union predates it.
      await game.entities.sendMessage(trigger.entity_id, {
        type: "TurnOn",
      } as unknown as Parameters<typeof game.entities.sendMessage>[1]);
      await game.step({ frames: 20 });
      // Each tour leaves on its authored shuttle movie
      // (campaign-cutscenes.e2e.test.ts); the year only advances on the far side.
      await stepPastCutscenes(game);
      mission = (await game.info()).mission;
    }

    assert.equal(
      mission.toLowerCase(),
      "medsci1.mis",
      `training should deploy to medsci1 (looped forever before the fix); ended at ${mission}`,
    );
  },
);
