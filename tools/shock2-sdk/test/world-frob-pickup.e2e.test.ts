import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

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

    // The circuit board is MOVE | SCRIPT. Take it through the production flat
    // crosshair/squeeze path: the controller must route the authored SCRIPT
    // action through FrobQB instead of bypassing it with a bare StoreItem.
    assert.equal(await game.quests.get("note_1_10"), "unknown");
    assert.equal(
      (await game.physics.bodies({ entityId: circuitBoard.id })).bodies.length,
      1,
      "the authored circuit board should begin as a physical world item",
    );
    const modulesBefore = (await game.info()).player.stats?.cyber_modules;
    assert.equal(modulesBefore, 0, "a fresh eng1 character starts with no modules");

    const [boardX, boardY, boardZ] = (
      await game.entities.detail(circuitBoard.id)
    ).position;
    await game.player.teleport({
      x: boardX + 0.2,
      y: boardY - PLAYER_EYE_HEIGHT_WORLD,
      z: boardZ,
    });
    const aim = await game.player.aimAt(circuitBoard, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(
      aim.target_confirmed,
      true,
      `the circuit board should expose a selectable surface: ${JSON.stringify(aim)}`,
    );
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 5 });

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
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      10,
      "the same single Frob should relay the authored +10 module reward exactly once",
    );
    assert.equal(
      (await game.physics.bodies({ entityId: circuitBoard.id })).bodies.length,
      0,
      "the collected circuit board should no longer have a world body",
    );

    const saveName = `world_frob_scripted_pickup_${Date.now()}`;
    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });

    const [loadedBoard] = await game.entities.byTemplate(CIRCUIT_BOARD_OBJ);
    assert.ok(loadedBoard, "save/load should restore the unique circuit board");
    assert.equal(
      (await game.player.inventory()).items.find(
        (entry) => entry.entity_id === loadedBoard.id,
      )?.location,
      "inventory",
      "save/load must preserve the scripted pickup in the backpack",
    );
    assert.equal(await game.quests.get("note_1_10"), "complete");
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      10,
      "save/load must preserve one reward without replaying the pickup",
    );
  },
);
