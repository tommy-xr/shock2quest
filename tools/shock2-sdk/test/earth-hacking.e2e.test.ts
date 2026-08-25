import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { teleportVerified } from "./helpers/teleport.js";

// Honest Earth Technical Training regression for #539. Runtime entity ids are
// deliberately never hardcoded: the authored mission-object ids below are
// stable `template_id` values, and the door is additionally verified through
// the keypad's SwitchLink.
//
// Negative-first evidence (2026-07-23): against integration base be38d04 this
// fails at "hackable keypad should expose the HRM start button". Physical
// pickup and physical keypad frob both succeed, but /v1/ui reports only the
// numeric 0-9/clear keypad because P$HackDiff was unparsed.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const EARTH_NANITES = 257;
const EARTH_HACKABLE_KEYPAD = 266;
const EARTH_LINKED_DOOR = 265;

test(
  "Earth hackable keypad: pay nanites, connect three HRM nodes, open linked door",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    const [nanites] = await game.entities.byTemplate(EARTH_NANITES);
    const [keypad] = await game.entities.byTemplate(EARTH_HACKABLE_KEYPAD);
    const [door] = await game.entities.byTemplate(EARTH_LINKED_DOOR);
    assert.ok(nanites, "Earth should contain authored nanite pile 257");
    assert.ok(keypad, "Earth should contain authored keypad 266");
    assert.ok(door, "Earth should contain authored door 265");

    const keypadDetail = await game.entities.detail(keypad.id);
    assert.ok(
      keypadDetail.outgoing_links.some(
        (link) =>
          link.link_type.toLowerCase().includes("switch") &&
          link.target_id === door.id,
      ),
      "keypad 266 should SwitchLink to door 265",
    );

    // Acquire the actual training nanites through the rendered world
    // interaction path (crosshair ray + right-hand squeeze), not debug give.
    await earthWorldUse(game, nanites);
    const inventoryBefore = await game.player.inventory();
    const carriedNanites = inventoryBefore.items.find(
      (item) => item.name === "Big Nanite Pile",
    );
    assert.ok(
      carriedNanites,
      `physical pickup should put Big Nanite Pile in the backpack (got ${JSON.stringify(inventoryBefore.items)})`,
    );
    const stackBefore = Number(
      (await game.entities.detail(carriedNanites.entity_id)).properties.find(
        (property) => property.name === "StackCount",
      )?.value,
    );
    assert.equal(stackBefore, 250, "the physical Earth pile should carry 250 nanites");

    const doorBefore = await game.entities.detail(door.id);

    // Frob through the same rendered-world input path. A direct debug Frob
    // message would not prove an ordinary player can reach the feature.
    const [keypadX, keypadY, keypadZ] = keypadDetail.position;
    await teleportVerified(game, {
      // Preserve the existing clear approach vector while keeping the
      // selectable surface inside retail's sqrt(50) / 2.5 frob reach.
      x: keypadX - 0.98,
      y: keypadY - 1,
      z: keypadZ + 1.95,
    });
    // Aim through the production look-at (it accounts for Earth's authored
    // body heading and reads the live camera height) rather than a fixed
    // pitch tuned to one camera position.
    const keypadAim = await game.player.aimAt(keypad, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      keypadAim.target_confirmed,
      true,
      `keypad staging must expose its selectable surface: ${JSON.stringify(keypadAim)}`,
    );
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
    await game.step({ frames: 5 });

    const opened = (await game.ui.state()).active_panel;
    assert.ok(opened, "physical keypad frob should open an MFD panel");
    assert.equal(opened.template_id, EARTH_HACKABLE_KEYPAD);
    const start = opened.elements.find(
      (element) =>
        element.kind === "button" && element.label === "start-hack",
    );
    assert.ok(start, "hackable keypad should expose the HRM start button");
    assert.ok(
      opened.elements.some(
        (element) =>
          element.texture?.toLowerCase() === "hack.pcx",
      ),
      "hackable keypad should render the retail HACK backdrop",
    );
    const costText = opened.elements
      .filter((element) => element.kind === "text")
      .map((element) => element.text ?? "")
      .find((text) => /^\d+$/.test(text));
    assert.ok(
      costText,
      "HRM panel should draw its authored numeric cost in HACK.PCX's COST slot",
    );
    const cost = Number(costText);
    assert.ok(cost > 0, `authored nanite cost should be positive (got ${costText})`);

    const clickElement = async (element: UiElement) => {
      const [x, y, width, height] = element.screen_rect;
      await game.input.set("pointer.position", [
        x + width / 2,
        y + height / 2,
      ]);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 1);
      await game.step({ frames: 2 });
      await game.input.set("pointer.pressed", 0);
      await game.step({ frames: 2 });
    };

    await clickElement(start);
    const stackAfterPayment = Number(
      (await game.entities.detail(carriedNanites.entity_id)).properties.find(
        (property) => property.name === "StackCount",
      )?.value,
    );
    assert.equal(
      stackAfterPayment,
      stackBefore - cost,
      "starting the board should deduct exactly the authored cost",
    );

    const doorDisplacement = async () => {
      const current = await game.entities.detail(door.id);
      return Math.hypot(
        current.position[0] - doorBefore.position[0],
        current.position[1] - doorBefore.position[1],
        current.position[2] - doorBefore.position[2],
      );
    };
    await game.step({ frames: 12 });
    assert.ok(
      (await doorDisplacement()) < 0.05,
      "paying the nanite cost alone must not open the linked door",
    );

    // The retail hacking layout's top row has three adjacent playable nodes
    // at x=2,3,4. The player must click all three; merely paying cannot open
    // the door.
    const path = ["node-2-0", "node-3-0", "node-4-0"];
    for (const [index, label] of path.entries()) {
      const panel = (await game.ui.state()).active_panel;
      assert.ok(panel, `HRM panel should remain open before clicking ${label}`);
      const node = panel.elements.find(
        (element) => element.kind === "button" && element.label === label,
      );
      assert.ok(node, `HRM board should expose playable ${label}`);
      await clickElement(node);
      if (index < path.length - 1) {
        await game.step({ frames: 12 });
        assert.ok(
          (await doorDisplacement()) < 0.05,
          `door 265 must remain closed after only ${index + 1} connected node(s)`,
        );
      }
    }

    const won = (await game.ui.state()).active_panel;
    assert.ok(won, "winning should leave the result panel visible");
    assert.ok(
      won.elements.some(
        (element) => element.texture?.toLowerCase() === "winh.pcx",
      ),
      `a connected three-node path should render the retail HACK success art; ` +
        `rng=${game.logs().filter((line) => line.includes("HRM rng")).join(" | ")}`,
    );
    assert.ok(
      won.elements.some(
        (element) => element.kind === "text" && element.text === costText,
      ),
      "the success overlay must not obscure the authored cost value",
    );

    await game.step({ frames: 120 });
    const doorMovement = await doorDisplacement();
    assert.ok(
      doorMovement > 1,
      `HRM success should TurnOn linked door 265 (moved ${doorMovement.toFixed(2)}; ` +
        `before=${doorBefore.position})`,
    );
  },
);
