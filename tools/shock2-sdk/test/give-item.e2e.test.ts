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
    });
    await game.step({ frames: 5 });

    // Cryo Card 1050 is an authored world-placed item with no incoming
    // Contains link (also used as the world-item control in container-loot).
    const worldItems = await game.entities.byTemplate(1050);
    assert.equal(worldItems.length, 1, "expected exactly one world-placed Cryo Card");
    const item = worldItems[0];
    assert.equal(item.location, "world", "the pickup should initially be in the world");

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

    const afterListing = await game.entities.list();
    const listedCarried = afterListing.entities.find((e) => e.id === item.id);
    assert.ok(listedCarried, "the carried item should remain discoverable by runtime id");
    assert.equal(
      listedCarried.location,
      "inventory",
      "the entity listing should identify a given item as carried",
    );
    assert.deepEqual(
      listedCarried.position,
      afterListing.player_position,
      "a carried item should not retain a stale floor position",
    );
    assert.equal(listedCarried.distance, 0, "a carried item is at the player, not on the floor");

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
    assert.equal(
      sameTemplate.find((e) => e.id === item.id)?.location,
      "inventory",
      "byTemplate should preserve the carried item's inventory state",
    );
  },
);

test(
  "give refuses container-held items without severing their containment",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
    });
    await game.step({ frames: 5 });

    // Male Corpse 1 (mission template 219) has exactly one authored item: a
    // Psi Amp. Runtime ids vary between launches, so discover it through the
    // stable container template and its live Contains link.
    const corpses = await game.entities.byTemplate(219);
    assert.equal(corpses.length, 1, "expected exactly one Male Corpse 1");
    const corpse = corpses[0];
    const before = await game.entities.detail(corpse.id);
    const contains = before.outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.equal(contains.length, 1, "corpse should contain exactly one item");
    const itemId = contains[0].target_id;

    // Negative-first regression for #572: main accepts this request, moves the
    // amp to the backpack, and permanently removes the corpse's Contains link.
    await assert.rejects(
      game.player.give(itemId),
      /status 400.*container-held.*loot it via the container MFD/is,
      "container-held items must be looted through the real container UI",
    );

    const after = await game.entities.detail(corpse.id);
    const preserved = after.outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.deepEqual(
      preserved,
      contains,
      "a rejected give must preserve the original Contains link exactly",
    );
    assert.ok(
      !(await game.player.inventory()).items.some((item) => item.entity_id === itemId),
      "a rejected give must not move the contained item into the backpack",
    );
  },
);
