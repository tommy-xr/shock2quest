import assert from "node:assert/strict";
import { test } from "node:test";

import { e2ePort } from "./helpers/e2e-port.js";
import { GameServer } from "../src/index.js";
import { stackCount } from "./helpers/nanites.js";

// Retail parity: picking up something that combines with an item already
// carried pools into that stack instead of claiming a cell of its own. The
// original gates this on a matching `P$CombineTy` label and merges BEFORE it
// adds the containment link, so a free cell never pre-empts the merge.
//
// Negative-first: on the parent, the merge is only a fallback for when
// placement fails, so with room in the backpack every assertion below sees two
// separate one-count entities instead of one pooled stack.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Gamesys templates (negative ids, stable across runs). */
const MED_PATCH = -52; // CombineType "MedPatch", stack 1.
const MEDICAL_KIT = -55; // CombineType "MedicalKit" - sibling of -52 under -51.
const SMALL_PRISM = -41; // CombineType "Prism", stack 10.
const LARGE_PRISM = -44; // CombineType "Prism", stack 20 - a DIFFERENT template.

/** The entity's authored `StackCount`, or undefined when it carries none. */
async function stackOf(game: GameServer, entityId: number): Promise<number | undefined> {
  return stackCount((await game.entities.detail(entityId)).properties);
}

/** Backpack entity ids, so a merge is visible as a drop in occupancy. */
async function carried(game: GameServer): Promise<number[]> {
  const { items } = await game.player.inventory();
  return items.map((item) => item.entity_id);
}

test(
  "a second med hypo pools into the first instead of taking its own cell",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: await e2ePort(),
    });
    await game.step({ frames: 10 });

    const before = await carried(game);
    const first = await game.player.spawnItem(MED_PATCH);
    assert.equal(
      await stackOf(game, first.entity_id),
      1,
      "a freshly provisioned med patch starts at one",
    );

    const second = await game.player.spawnItem(MED_PATCH);
    const after = await carried(game);

    assert.equal(
      after.length,
      before.length + 1,
      "two med patches must occupy ONE cell, not two",
    );
    assert.ok(
      after.includes(first.entity_id),
      "the first patch is the surviving stack",
    );
    assert.equal(
      second.entity_id,
      first.entity_id,
      "provisioning reports the stack that was merged into, not the destroyed donor",
    );
    assert.equal(
      await stackOf(game, first.entity_id),
      2,
      "the survivor's count absorbs the donor's",
    );
  },
);

test(
  "sibling archetypes sharing a combine label pool, differing labels do not",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: await e2ePort(),
    });
    await game.step({ frames: 10 });

    // Small and Large Prism are separate templates sharing "Prism", so the
    // label - not the template - has to carry the match. 10 + 20 = 30.
    const small = await game.player.spawnItem(SMALL_PRISM);
    const large = await game.player.spawnItem(LARGE_PRISM);
    const withPrisms = await carried(game);

    assert.equal(
      large.entity_id,
      small.entity_id,
      "a Large Prism pools into a carried Small Prism despite the differing template",
    );
    assert.ok(
      withPrisms.includes(small.entity_id),
      "the pooled prism stack is carried",
    );
    assert.equal(
      await stackOf(game, small.entity_id),
      30,
      "the pooled stack sums both authored counts",
    );

    // Med Patch and Medical Kit share the parent archetype -51 but carry
    // different labels, so a parent-based match would wrongly fuse them.
    const patch = await game.player.spawnItem(MED_PATCH);
    const kit = await game.player.spawnItem(MEDICAL_KIT);
    const withBoth = await carried(game);

    assert.ok(
      withBoth.includes(patch.entity_id) && withBoth.includes(kit.entity_id),
      "a med patch and a medical kit stay separate items",
    );
  },
);
