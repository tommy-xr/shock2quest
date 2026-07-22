import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the SimpleLevelChangeButton script (GitHub #522).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: "simplelevelchangebutton" used to map to UnimplementedScript,
// so frobbing the rec1 tram button (and the Rickenbacker shuttle buttons) did
// NOTHING - the genuine endgame (Command / Rickenbacker / Many / Shodan) was
// unreachable. This frobs the button and asserts the level actually transitions.
//
// SimpleLevelChangeButton is the ungated frob-to-travel variant of the Level
// Change Button: objects override their template's LevelChangeButton with it,
// but both read the object's PropDestLevel/PropDestLoc and transition on frob -
// so the fix binds it to the same LevelChangeButton script.
//
// Runtime entity ids are not stable across runs, so discover the button by its
// stable *mission object id* each launch. The debug runtime reports that object
// id in the `template_id` field of /v1/entities (it matches dark_query's object
// id space and is stable across runs, unlike the runtime entity id), so
// entities.byTemplate(<object id>) is a durable handle - never hardcode a
// runtime entity id.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "rec1 tram SimpleLevelChangeButton frob transitions to command1",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8123),
    });
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).mission,
      "rec1.mis",
      "should start in rec1",
    );

    // The rec1 tram button is mission object 664 (script SimpleLevelChangeButton,
    // PropDestLevel=command1). Discover it by that stable object id.
    const buttons = await game.entities.byTemplate(664);
    assert.equal(
      buttons.length,
      1,
      `expected the rec1 tram button (object 664), got ${JSON.stringify(buttons.map((b) => b.name))}`,
    );

    // Frob it - the script emits GlobalEffect::TransitionLevel to command1.
    await game.entities.sendMessage(buttons[0].id, { type: "Frob" });
    await game.step({ frames: 15 });

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "command1.mis",
      "frobbing the rec1 tram button should transition to command1 " +
        "(if this stays rec1.mis, SimpleLevelChangeButton is stubbed again)",
    );
  },
);

test(
  "rick3 Shuttle_Button SimpleLevelChangeButton frob transitions to Many",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rick3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8124),
    });
    await game.step({ frames: 2 });

    // rick3 has two Level Change Buttons; the shuttle is mission object 104
    // (script SimpleLevelChangeButton, PropDestLevel=Many). The other (183) is a
    // plain LevelChangeButton. Discover the shuttle by its stable object id.
    const buttons = await game.entities.byTemplate(104);
    assert.equal(
      buttons.length,
      1,
      `expected the rick3 shuttle button (object 104), got ${JSON.stringify(buttons.map((b) => b.name))}`,
    );

    await game.entities.sendMessage(buttons[0].id, { type: "Frob" });
    await game.step({ frames: 15 });

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "many.mis",
      "frobbing the rick3 shuttle button should transition to Many",
    );
  },
);
