import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "earth: flat world-use stores ammo without displacing the wielded pistol",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 30 });

    // PropTemplateId is the stable mission-object id for level-authored
    // entities. Runtime ids are deliberately rediscovered every launch.
    const entities = (await game.entities.list()).entities;
    const pistol = entities.find(
      (entity) => entity.template_id === 246 && entity.name === "Pistol",
    );
    const clip = entities.find(
      (entity) =>
        entity.template_id === 249 && entity.name.includes("Standard Clip"),
    );
    assert.ok(pistol, "expected Earth Weapons Training pistol 246");
    assert.ok(clip, "expected Earth Weapons Training standard clip 249");
    assert.equal(pistol.location, "world", "the pistol should initially be in the world");
    assert.equal(clip.location, "world", "the clip should initially be in the world");

    await earthWorldUse(game, pistol);
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      pistol.id,
      "normal world-use should auto-wield the pistol",
    );

    await earthWorldUse(game, clip);

    const player = (await game.info()).player;
    assert.equal(
      player.wielded_entity_id,
      pistol.id,
      "picking up ammo must retain the wielded pistol",
    );
    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === pistol.id)?.location,
      "left_hand",
      `pistol should remain wielded, got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      inventory.items.find((item) => item.entity_id === clip.id)?.location,
      "inventory",
      `clip should land in the backpack, got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: clip.id })).bodies.length,
      0,
      "a backpack item must no longer have a world physics body",
    );

    // Entity discovery remains available after the real aim+squeeze pickup,
    // but its spatial state must follow the player rather than the old shelf.
    const listing = await game.entities.list();
    const listedPistol = listing.entities.find((entity) => entity.id === pistol.id);
    const listedClip = listing.entities.find((entity) => entity.id === clip.id);
    assert.equal(listedPistol?.location, "left_hand");
    assert.equal(listedClip?.location, "inventory");
    assert.ok(listedPistol, "the held pistol should remain discoverable by runtime id");
    assert.ok(
      listedPistol.position.every(Number.isFinite),
      "the held pistol should retain its live viewmodel position",
    );
    assert.ok(listedClip, "the backpack clip should remain discoverable by runtime id");
    assert.deepEqual(listedClip.position, listing.player_position);
    assert.equal(listedClip.distance, 0);
  },
);
