import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for switch-driven level-change buttons (GitHub #555).
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: LevelChangeButton used to handle only `Frob`, dropping a
// `TurnOn` that arrived over a SwitchLink. On the Operations deck the button
// the player actually touches relays through a quest-bit filter to a hidden
// co-located changer object that only ever receives `TurnOn`, so every Ops
// bulkhead was sealed and the deck was unfinishable.
//
// ops2 object 185 is the visible ops3 bulkhead button; it relays
// 185 -> 995 (QB Filter on ShodanRoom) -> 992 (the changer, dest ops3).
// Runtime entity ids are not stable across runs, so discover the button by its
// stable *mission object id*, reported in the `template_id` field.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "ops2: frobbing the visible bulkhead button relays TurnOn and transitions to ops3",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8125),
    });
    await game.step({ frames: 2 });
    assert.equal(
      (await game.info()).mission,
      "ops2.mis",
      "should start in ops2",
    );

    // The relay is gated on the ShodanRoom reveal (QB Filter 995).
    await game.quests.set("ShodanRoom", "complete");

    const buttons = await game.entities.byTemplate(185);
    assert.equal(
      buttons.length,
      1,
      `expected the ops2 ops3-bulkhead button (object 185), got ${JSON.stringify(buttons.map((b) => b.name))}`,
    );

    await game.entities.sendMessage(buttons[0].id, { type: "Frob" });
    await game.step({ frames: 30 });

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "ops3.mis",
      "frobbing the visible ops2 bulkhead button should reach the hidden " +
        "changer over its switch links and transition to ops3",
    );
  },
);

test(
  "ops3: a directly-frobbed level change button still transitions",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops3.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8126),
    });
    await game.step({ frames: 2 });

    // ops3 object 742 carries LevelChangeButton with no switch links at all -
    // the regression guard that routing frob through BaseButton (frob -> TurnOn
    // to self) keeps directly-frobbed buttons working.
    const buttons = await game.entities.byTemplate(742);
    assert.equal(
      buttons.length,
      1,
      `expected the ops3 ops2-bulkhead button (object 742), got ${JSON.stringify(buttons.map((b) => b.name))}`,
    );

    await game.entities.sendMessage(buttons[0].id, { type: "Frob" });
    await game.step({ frames: 30 });

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "ops2.mis",
      "frobbing a level change button directly should still transition",
    );
  },
);
