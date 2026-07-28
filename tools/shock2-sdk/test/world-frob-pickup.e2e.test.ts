import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable eng1 mission-object ids. Runtime entity ids are rediscovered every
// launch and must never be hardcoded.
const VACC_SUIT_OBJ = 1450;
const MED_PATCH_OBJ = 1318;
const CIRCUIT_BOARD_OBJ = 705;

test(
  "eng1: frobbing MOVE items picks them up and preserves the scripted circuit board flow",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "eng1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8192),
    });
    await game.step({ frames: 2 });

    const [vaccSuit] = await game.entities.byTemplate(VACC_SUIT_OBJ);
    const [medPatch] = await game.entities.byTemplate(MED_PATCH_OBJ);
    const [circuitBoard] = await game.entities.byTemplate(CIRCUIT_BOARD_OBJ);
    assert.ok(vaccSuit?.name === "Vacc Suit", "expected eng1 Vacc Suit obj 1450");
    assert.ok(medPatch?.name === "Med Patch", "expected eng1 Med Patch obj 1318");
    assert.ok(
      circuitBoard?.name === "Circuitboard",
      "expected eng1 circuit board obj 705",
    );

    // The suit and patch inherit Goodies' world-action MOVE, with no SCRIPT
    // world action to perform that transfer for them. Before #591 an injected
    // Frob only reached their ordinary scripts and both remained uncarried.
    for (const item of [vaccSuit, medPatch]) {
      await game.entities.sendMessage(item.id, { type: "Frob" });
      await game.step({ frames: 2 });
      const inventory = await game.player.inventory();
      assert.equal(
        inventory.items.find((entry) => entry.entity_id === item.id)?.location,
        "inventory",
        `frobbing ${item.name} must honor PropFrobInfo MOVE; got ${JSON.stringify(inventory.items)}`,
      );
    }

    // The circuit board is MOVE | SCRIPT. Its existing FrobQB path must still
    // both transfer the unique board and award the authored quest bit.
    await game.entities.sendMessage(circuitBoard.id, { type: "Frob" });
    await game.step({ frames: 2 });
    const inventory = await game.player.inventory();
    assert.equal(
      inventory.items.find((entry) => entry.entity_id === circuitBoard.id)
        ?.location,
      "inventory",
      `circuit board must remain pickable; got ${JSON.stringify(inventory.items)}`,
    );
    assert.equal(
      await game.quests.get("note_1_10"),
      "complete",
      "circuit board FrobQB must still award note_1_10",
    );
  },
);
