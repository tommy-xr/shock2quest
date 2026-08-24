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

// The player pose observed in #827 immediately before the ordinary production
// squeeze. The button position itself comes from the runtime entity so the
// interaction follows the authored mission object rather than a debug Frob.
const OPS4_BUTTON_PLAYER_POSE = {
  x: 53.47933,
  y: -7.8612247,
  z: -6.4060183,
};

async function squeezeVisibleOps4Button(game: GameServer): Promise<void> {
  const [button] = await game.entities.byTemplate(404);
  assert.ok(button, "expected the visible ops2 Ops 4 bulkhead button");

  await game.player.teleport(OPS4_BUTTON_PLAYER_POSE);
  await game.step({ frames: 2 });
  await game.input.lookAtWorldPoint(button.position);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
  await game.step({ frames: 30 });
}

test(
  "ops2: the visible Ops 4 button cannot directly select its hidden pre-reveal relay",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
    });
    await game.step({ frames: 2 });
    assert.equal(await game.quests.get("ShodanRoom"), "unknown");

    // ops2 object 999 is the NoRender, PickBias=-2000 level-change relay.
    // Dark never offers NoRender objects to the render-driven pick system, so
    // an object with no authored PhysType must not gain a synthetic frob body.
    const [hiddenRelay] = await game.entities.byTemplate(999);
    assert.ok(hiddenRelay, "expected the hidden ops2 Ops 4 relay");
    assert.equal(
      (await game.physics.bodies({ entityId: hiddenRelay.id })).total_count,
      0,
      "the NoRender relay must remain script-addressable without becoming a direct interaction target",
    );

    await squeezeVisibleOps4Button(game);

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "ops2.mis",
      "the visible button's authored QB filter must block Ops 4 before the SHODAN reveal",
    );
  },
);

test(
  "ops1 reveal keeps its NoRender sensor and unlocks the visible ops2 Ops 4 button",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops1.mis",
    });
    await game.step({ frames: 2 });

    // Enter the real reveal tripwire (ops1 object 204). It is also NoRender,
    // but unlike the hidden relay it has an authored PhysType and therefore
    // must keep the sensor body that drives TrapNewTripwire.
    const [revealTripwire] = await game.entities.byTemplate(204);
    assert.ok(revealTripwire, "expected the ops1 SHODAN reveal tripwire");
    const revealBodies = await game.physics.bodies({
      entityId: revealTripwire.id,
    });
    assert.ok(
      revealBodies.bodies.some((body) => body.is_sensor),
      "the authored NoRender reveal sensor must remain physical",
    );

    await game.player.teleport({
      x: revealTripwire.position[0] + 2,
      y: revealTripwire.position[1],
      z: revealTripwire.position[2],
    });
    await game.step({ frames: 2 });
    await game.player.teleport({
      x: revealTripwire.position[0],
      y: revealTripwire.position[1],
      z: revealTripwire.position[2],
    });
    await game.step({ frames: 5 });
    assert.equal(
      await game.quests.get("ShodanRoom"),
      "incomplete",
      "entering the authored reveal tripwire should establish ShodanRoom",
    );

    // Preserve the quest state while returning to ops2; the interaction below
    // is still the ordinary production aim/squeeze and the hidden changer is
    // reached only through visible button -> QB filter -> SwitchLink.
    await game.transitionLevel("ops2.mis");
    await game.step({ frames: 2 });
    await squeezeVisibleOps4Button(game);

    assert.equal(
      (await game.info()).mission.toLowerCase(),
      "ops4.mis",
      "after the genuine reveal flow, the authored visible-button relay chain should reach Ops 4",
    );
  },
);

test(
  "ops2: frobbing the visible bulkhead button relays TurnOn and transitions to ops3",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "ops2.mis",
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
