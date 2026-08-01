import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable command2 mission-object ids. Runtime entity ids are rediscovered on
// every launch and are never used as durable identities.
const BRIDGE_CARD = 135;
const CARD_SLOT = 138;
const BRIDGE_DOOR = 144;
const EYE_HEIGHT = 1.6;

async function only(game: GameServer, missionObjectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(missionObjectId);
  assert.equal(
    found.length,
    1,
    `expected one command2 object ${missionObjectId}, got ${JSON.stringify(found)}`,
  );
  return found[0];
}

/** Stage near an authored object, acquire a verified crosshair target, and use
 * the normal flat squeeze edge. The candidate offsets handle which side of a
 * wall-mounted object is open without weakening the required visibility check. */
async function worldUse(game: GameServer, target: EntitySummary): Promise<void> {
  const [x, y, z] = (await game.entities.detail(target.id)).position;
  const offsets = [
    [0, 1.2],
    [0, -1.2],
    [1.2, 0],
    [-1.2, 0],
  ] as const;

  let lastError: unknown;
  for (const [dx, dz] of offsets) {
    await game.player.teleport({ x: x + dx, y: y - EYE_HEIGHT, z: z + dz });
    try {
      const aim = await game.player.aimAt(target, {
        hitbox: "center",
        visibility: "required",
      });
      if (!aim.target_confirmed) {
        continue;
      }
      await game.input.set("right_hand.squeeze_value", 1);
      await game.step({ frames: 2 });
      await game.input.set("right_hand.squeeze_value", 0);
      await game.step({ frames: 2 });
      return;
    } catch (error) {
      lastError = error;
    }
  }

  throw new Error(
    `could not stage a visible use ray to ${target.name} (${target.template_id}): ${String(lastError)}`,
  );
}

test(
  "command2: picking up the Bridge Card grants access to its authored slot",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8201),
    });
    await game.step({ frames: 5 });

    const bridgeCard = await only(game, BRIDGE_CARD);
    const cardSlot = await only(game, CARD_SLOT);
    const bridgeDoor = await only(game, BRIDGE_DOOR);
    assert.equal(bridgeCard.name, "Bridge Card");
    assert.equal(cardSlot.name, "Card slot");
    assert.equal(bridgeDoor.name, "Security Door");

    const doorBefore = await game.entities.detail(bridgeDoor.id);

    // This is the production flat pickup path that regressed: object 135 is
    // authored MOVE-only, but PropKeySrc must still run internal_keycard.
    await worldUse(game, bridgeCard);
    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((item) => item.entity_id === bridgeCard.id)?.location,
      "inventory",
      `the physical Bridge Card must remain in the backpack: ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      (await game.physics.bodies({ entityId: bridgeCard.id })).bodies.length,
      0,
      "the stored Bridge Card must no longer have a world body",
    );

    // Slot 138 requires KeyDst region 8192 and SwitchLinks to door 144. Its
    // normal use proves that pickup acquired the matching persistent key state.
    await worldUse(game, cardSlot);
    await game.step({ frames: 120 });

    const doorAfter = await game.entities.detail(bridgeDoor.id);
    assert.ok(
      Math.abs(doorAfter.position[1] - doorBefore.position[1]) > 1,
      `the Bridge Card slot must open door 144, before=${JSON.stringify(doorBefore.position)}, after=${JSON.stringify(doorAfter.position)}`,
    );
  },
);
