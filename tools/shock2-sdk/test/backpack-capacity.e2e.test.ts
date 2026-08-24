import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, HttpError } from "../src/index.js";
import type { UiElement } from "../src/types.js";
import { earthWorldUse } from "./helpers/earth-world-use.js";
import { teleportVerified } from "./helpers/teleport.js";
import { aimVrHandAt, aimVrHandAtCanvas } from "./helpers/vr-hand.js";

// Retail parity: a full backpack REJECTS a transfer into it (panel Take,
// flat/VR world pickup, VR strip deposit) rather than silently absorbing the
// item with no cell to remember it by. The item stays exactly where it was.
// A full container that still holds a matching stack (same template, both
// stackable) merges into it instead of refusing.
//
// Negative-first: on the parent, `move_live_entity_into_container`'s
// first-free fallback forces slot 0 regardless of capacity, so every "must
// stay put" assertion below fails - the item lands in the backpack anyway.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Gamesys templates (negative ids, stable across runs). */
const WRENCH_TEMPLATE = -928; // 1x3 footprint, not stackable - fills a whole column.
const CLIP_TEMPLATE = -1358; // "Small Standard Clip", 1x1, stackable.

/** earth.mis stable mission-object ids (see flat-world-pickup.e2e.test.ts,
 * nanite-player-stat.e2e.test.ts, vr-cyber-interface-deposit.e2e.test.ts). */
const EARTH_CLIP_OBJ = 249; // Standard Clip, template -1358 - a world pickup.
const EARTH_NANITE_PILE_OBJ = 257; // Big Nanite Pile - always-collected.

/** medsci1.mis stable mission-object id (see container-loot.e2e.test.ts). */
const MEDSCI_WRENCH_CORPSE = 1177; // MS Male Corpse -> Contains -> Wrench.

/**
 * Fill the player's backpack completely with non-stackable Wrenches (1x3):
 * each spawn claims a whole grid column, so once every column is taken the
 * next spawn has nowhere to go and genuinely refuses - independent of the
 * backpack's actual width (Strength-dependent), no hardcoded cell count.
 */
async function fillBackpackWithWrenches(game: GameServer): Promise<number> {
  let count = 0;
  for (let i = 0; i < 40; i++) {
    try {
      await game.player.spawnItem(WRENCH_TEMPLATE);
      count++;
    } catch (error) {
      assert.ok(
        error instanceof HttpError && error.status === 400,
        `expected a refusal (400), got ${error}`,
      );
      return count;
    }
  }
  throw new Error("backpack never became full after 40 wrenches");
}

test(
  "a full backpack still merges a matching stackable instead of refusing it",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9501),
    });
    await game.step({ frames: 2 });

    // Spawn 1x1 Standard Clips one at a time until the backpack stops
    // growing a new distinct item - the point at which every cell is
    // already a Standard Clip stack and the next one can only merge. On the
    // parent this loop never plateaus (a "full" backpack still silently
    // takes every item at slot 0), so it exhausts its budget and throws.
    let previousCount = (await game.player.inventory()).count;
    let plateaued = false;
    for (let i = 0; i < 60 && !plateaued; i++) {
      await game.player.spawnItem(CLIP_TEMPLATE);
      const currentCount = (await game.player.inventory()).count;
      if (currentCount === previousCount) {
        plateaued = true;
      } else {
        previousCount = currentCount;
      }
    }
    assert.ok(
      plateaued,
      "the backpack should fill with distinct clip stacks and then start merging",
    );
    assert.ok(previousCount >= 10, `expected a plausible backpack size, got ${previousCount}`);

    // The merge must not have refused the deposit: the spawned entity was
    // consumed into an existing stack, so the item count is unchanged and
    // every carried item is still a Standard Clip.
    const inventory = await game.player.inventory();
    assert.equal(inventory.count, previousCount);
    assert.ok(
      inventory.items.every((item) => item.name?.includes("Clip")),
      `expected every carried item to be a Clip, got ${JSON.stringify(inventory.items)}`,
    );
  },
);

test(
  "a full backpack refuses a panel Take: the item stays in its container",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9501) + 1,
    });
    await game.step({ frames: 5 });

    const filled = await fillBackpackWithWrenches(game);
    assert.ok(filled > 0, "expected at least one wrench to fit initially");
    const beforeCount = (await game.player.inventory()).count;

    const [corpse] = await game.entities.byTemplate(MEDSCI_WRENCH_CORPSE);
    assert.ok(corpse, "expected MS Male Corpse 1177");
    const containsBefore = (await game.entities.detail(corpse.id)).outgoing_links.filter(
      (l) => l.link_type.startsWith("Contains"),
    );
    assert.equal(containsBefore.length, 1, "corpse 1177 should contain exactly the Wrench");
    const lootWrenchId = containsBefore[0].target_id;

    await teleportVerified(game, {
      x: corpse.position[0] + 1.0,
      y: corpse.position[1] + 0.5,
      z: corpse.position[2] + 1.0,
    });
    await game.entities.sendMessage(corpse.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const panel = (await game.ui.state()).active_panel;
    assert.ok(panel, "frobbing the corpse should open its loot MFD panel");
    const wrenchElement = panel!.elements.find(
      (e: UiElement) => e.kind === "button" && e.entity_id === lootWrenchId,
    );
    assert.ok(
      wrenchElement,
      `loot panel should list the Wrench: ${JSON.stringify(panel!.elements)}`,
    );

    const [x, y, w, h] = wrenchElement.screen_rect;
    const center: [number, number] = [x + w / 2, y + h / 2];
    await game.input.set("pointer.position", center);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 1);
    await game.step({ frames: 2 });
    await game.input.set("pointer.pressed", 0);
    await game.step({ frames: 5 });

    // The refused item must NOT be in the backpack, count unchanged, and it
    // must still be exactly where it was: contained by the corpse, no world
    // physics presence, still listed in the (still open) loot panel.
    assert.equal(
      (await game.player.inventory()).items.find((i) => i.entity_id === lootWrenchId),
      undefined,
      "a refused Take must not add the item to the backpack",
    );
    assert.equal((await game.player.inventory()).count, beforeCount, "item count must be unchanged");
    const containsAfter = (await game.entities.detail(corpse.id)).outgoing_links.filter(
      (l) => l.link_type.startsWith("Contains"),
    );
    assert.equal(
      containsAfter.length,
      1,
      "the refused Wrench must still be contained by the corpse",
    );
    assert.equal(containsAfter[0].target_id, lootWrenchId);
    assert.equal(
      (await game.physics.bodies({ entityId: lootWrenchId })).bodies.length,
      0,
      "the refused item stays contained, not spilled into the world",
    );
    const stillOpen = (await game.ui.state()).active_panel;
    assert.ok(
      stillOpen?.elements.some((e) => e.entity_id === lootWrenchId),
      "the refused Wrench must still be listed in the loot panel",
    );
  },
);

test(
  "a full backpack refuses a flat world pickup: the item stays in the world",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9501) + 2,
    });
    await game.step({ frames: 30 });

    await fillBackpackWithWrenches(game);
    const beforeCount = (await game.player.inventory()).count;

    const entities = (await game.entities.list()).entities;
    const clip = entities.find(
      (entity) => entity.template_id === EARTH_CLIP_OBJ && entity.name.includes("Standard Clip"),
    );
    assert.ok(clip, "expected earth Weapons Training standard clip 249");

    await earthWorldUse(game, clip!);

    assert.equal(
      (await game.player.inventory()).items.find((i) => i.entity_id === clip!.id),
      undefined,
      "the refused clip must not enter the backpack",
    );
    assert.equal((await game.player.inventory()).count, beforeCount, "item count must be unchanged");
    assert.ok(
      (await game.physics.bodies({ entityId: clip!.id })).bodies.length > 0,
      "the refused clip must remain a loose world prop",
    );
  },
);

test(
  "a full backpack still collects an always-collected nanite pile",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9501) + 3,
    });
    await game.step({ frames: 2 });

    await fillBackpackWithWrenches(game);
    const statsBefore = (await game.info()).player.stats;
    assert.equal(statsBefore?.nanites ?? 0, 0, "a fresh earth character starts with no nanites");

    const [pile] = await game.entities.byTemplate(EARTH_NANITE_PILE_OBJ);
    assert.ok(pile, "expected earth mission object 257 (Big Nanite Pile)");
    const stackProp = (await game.entities.detail(pile.id)).properties.find((p) =>
      p.name.includes("StackCount"),
    );
    const expectedNanites = Number(stackProp?.value ?? "0");
    assert.ok(expectedNanites > 0, "expected a positive authored stack count");

    await game.entities.sendMessage(pile.id, { type: "Frob" });
    await game.step({ frames: 5 });

    assert.equal(
      (await game.info()).player.stats?.nanites,
      expectedNanites,
      "the pile must still collect as a stat even with a full backpack",
    );
    assert.equal(
      (await game.player.inventory()).items.find((i) => i.entity_id === pile.id),
      undefined,
      "a collected nanite pile must never enter the inventory grid",
    );
  },
);

test(
  "a full backpack forces a VR strip release back to the ordinary world drop",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 9501) + 4,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    await fillBackpackWithWrenches(game);
    const beforeCount = (await game.player.inventory()).count;

    const clip = (await game.entities.list()).entities.find(
      (entity) => entity.template_id === EARTH_CLIP_OBJ,
    );
    assert.ok(clip, "expected earth mission object 249 (Standard Clip)");
    assert.ok(
      (await game.physics.bodies({ entityId: clip!.id })).bodies.length > 0,
      "the clip must start as a physical world prop",
    );

    const [x, y, z] = clip!.position;
    await game.player.teleport({ x: x + 1.2, y: y - 0.8, z: z + 1.2 });
    await game.step({ frames: 30 });
    const aim = await game.player.aimAt(clip!.id, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
    await aimVrHandAt(game, aim.world_point, 0.35, 1);
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.right_hand_entity_id,
      clip!.id,
      "the world squeeze must put the clip in the hand",
    );

    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const pose = (await game.ui.state()).panel_pose!;
    assert.ok(pose, "the open interface must report its panel pose");

    // Aim the holding hand at the strip and release - the deposit target -
    // with a full backpack behind it.
    const STRIP_CANVAS: [number, number] = [320, 60];
    await aimVrHandAtCanvas(game, pose, STRIP_CANVAS, { squeeze: 1 });
    await game.step({ frames: 5 });
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 10 });

    // The hand must still empty (the item leaves it either way), but a full
    // backpack means this is the ordinary world drop, not a deposit: no
    // backpack entry, and the item lands back in the world with a body.
    assert.equal((await game.info()).player.right_hand_entity_id, null);
    assert.equal(
      (await game.player.inventory()).items.find((i) => i.entity_id === clip!.id),
      undefined,
      "a refused strip deposit must not add the item to the backpack",
    );
    assert.equal((await game.player.inventory()).count, beforeCount, "item count must be unchanged");
    assert.ok(
      (await game.physics.bodies({ entityId: clip!.id })).bodies.length > 0,
      "a refused strip release must fall back to the ordinary world drop",
    );
  },
);
