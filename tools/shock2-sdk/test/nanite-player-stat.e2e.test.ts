import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";
import { stackCount as rawStackCount } from "./helpers/nanites.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable earth.mis mission-object ids (see nanite-stack-label.e2e.test.ts).
// Both are "Big Nanite Pile" instances so frobbing one leaves the other
// available to exercise squeeze separately.
const FROB_PILE_OBJ = 257;
const SQUEEZE_PILE_OBJ = 292;

function stackCount(properties: { name: string; value: string }[]): number {
  const stack = rawStackCount(properties);
  assert.ok(stack !== undefined, "expected an authored StackCount");
  return stack;
}

test(
  "earth: frobbing and squeezing a world nanite pickup collects it as a player stat, never inventory",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8194),
    });
    await game.step({ frames: 2 });

    const statsBefore = (await game.info()).player.stats;
    assert.equal(
      statsBefore?.nanites ?? 0,
      0,
      "a fresh earth character starts with no stat nanites",
    );

    // --- Frob ---
    const [frobPile] = await game.entities.byTemplate(FROB_PILE_OBJ);
    assert.ok(
      frobPile,
      `expected earth mission object ${FROB_PILE_OBJ} (Big Nanite Pile)`,
    );
    const frobStack = stackCount(
      (await game.entities.detail(frobPile.id)).properties,
    );
    assert.ok(frobStack > 0, "expected a positive authored stack count");

    await game.entities.sendMessage(frobPile.id, { type: "Frob" });
    await game.step({ frames: 5 });

    const afterFrob = (await game.info()).player.stats;
    assert.equal(
      afterFrob?.nanites,
      frobStack,
      "Frob should award the pile's full stack straight to the nanite stat",
    );
    assert.equal(
      (await game.player.inventory()).items.find(
        (entry) => entry.entity_id === frobPile.id,
      ),
      undefined,
      "a collected nanite pile must never enter the inventory grid",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: frobPile.id })).bodies.length,
      0,
      "the collected pile should no longer have a world body",
    );

    // --- Squeeze (the VR grab gesture, routed through virtual_hand) ---
    const [squeezePile] = await game.entities.byTemplate(SQUEEZE_PILE_OBJ);
    assert.ok(
      squeezePile,
      `expected earth mission object ${SQUEEZE_PILE_OBJ} (Big Nanite Pile)`,
    );
    const squeezeStack = stackCount(
      (await game.entities.detail(squeezePile.id)).properties,
    );

    const [x, y, z] = (await game.entities.detail(squeezePile.id)).position;
    await game.player.teleport({
      x,
      y: y - PLAYER_EYE_HEIGHT_WORLD,
      z: z + 0.3,
    });
    const aim = await game.player.aimAt(squeezePile, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      aim.target_confirmed,
      true,
      `the pile should be selectable for squeeze: ${JSON.stringify(aim)}`,
    );

    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 5 });

    const afterSqueeze = (await game.info()).player.stats;
    assert.equal(
      afterSqueeze?.nanites,
      frobStack + squeezeStack,
      "squeezing a nanite pile must also collect it straight into the stat",
    );
    assert.equal(
      (await game.player.inventory()).items.find(
        (entry) => entry.entity_id === squeezePile.id,
      ),
      undefined,
      "squeezing a nanite pile must never place it in the hand or the inventory grid",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: squeezePile.id })).bodies.length,
      0,
      "the squeezed pile should no longer have a world body",
    );
  },
);
