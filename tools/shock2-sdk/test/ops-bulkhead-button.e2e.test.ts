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

test(
  "ops4: aimAt center selects the offset-pivot button and production squeeze opens its doors",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops4.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8127),
    });
    await game.step({ frames: 2 });

    const buttons = await game.entities.byTemplate(334);
    assert.equal(buttons.length, 1, "expected the offset-pivot door button");
    const [leftDoor] = await game.entities.byTemplate(331);
    const [rightDoor] = await game.entities.byTemplate(332);
    assert.ok(leftDoor && rightDoor, "expected both linked Ops doors");
    await game.player.teleport({ x: 53.5, y: -9.796, z: -60.8 });

    const aim = await game.player.aimAt(buttons[0], { hitbox: "center" });
    assert.equal(aim.classification, "surface");
    assert.equal(aim.entity_id, buttons[0].id);
    assert.equal(aim.interaction_target_id, buttons[0].id);
    assert.equal(aim.target_confirmed, true);

    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 120 });

    const [openedLeft, openedRight] = await Promise.all([
      game.entities.detail(leftDoor.id),
      game.entities.detail(rightDoor.id),
    ]);
    assert.ok(
      Math.abs(openedLeft.position[0] - leftDoor.position[0]) > 1,
      "left linked door should translate open",
    );
    assert.ok(
      Math.abs(openedRight.position[0] - rightDoor.position[0]) > 1,
      "right linked door should translate open",
    );
  },
);
