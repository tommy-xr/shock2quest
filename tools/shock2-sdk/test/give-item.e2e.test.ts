import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the give-item (headless pickup) lever. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before POST /v1/player/give existed, there was no headless
// way to get an existing world item into the player's inventory (pickup runs
// through the GUI/grab flow). This gives a world item to the player and asserts
// it lands in the backpack, verifiable via /v1/player/inventory.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "give puts an existing world item into the player's inventory",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8104),
    });
    await game.step({ frames: 5 });

    // Find a pickup item in the level (a nanite stack) by name.
    const { entities } = await game.entities.list({
      filter: "*Nanites*",
      limit: 50,
    });
    const item = entities[0];
    assert.ok(item, "expected to find a Nanites pickup in medsci1");

    // It should not be carried yet.
    const before = await game.player.inventory();
    assert.ok(
      !before.items.some((i) => i.entity_id === item.id),
      "item should not start in the inventory",
    );

    // Give it, then confirm it's now in the backpack.
    const result = await game.player.give(item.id);
    assert.equal(result.success, true);

    const after = await game.player.inventory();
    const carried = after.items.find((i) => i.entity_id === item.id);
    assert.ok(carried, `item ${item.id} should be carried after give`);
    assert.equal(
      carried.location,
      "inventory",
      "a given item lands in the backpack, not a hand",
    );

    // A bad id is rejected without crashing the runtime.
    await assert.rejects(
      game.player.give(999_999),
      /status 400|not alive|invalid entity/,
      "giving a nonexistent entity should error",
    );

    // A non-pickup entity (a door) is rejected too - give only accepts genuine
    // pickup items, so it can't reparent structural entities into the backpack.
    const doors = await game.entities.list({ filter: "*Door*", limit: 50 });
    const door = doors.entities[0];
    if (door) {
      await assert.rejects(
        game.player.give(door.id),
        /status 400|not a pickup/,
        "giving a non-pickup entity (door) should error",
      );
    }
    assert.ok(
      (await game.player.inventory()).items.some((i) => i.entity_id === item.id),
      "runtime should stay live and keep the item after a rejected give",
    );

    // byTemplate maps template -> runtime id: the item we found is an instance
    // of its own template.
    const sameTemplate = await game.entities.byTemplate(item.template_id);
    assert.ok(
      sameTemplate.some((e) => e.id === item.id),
      "byTemplate should include the item among its template's instances",
    );
  },
);
