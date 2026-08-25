import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";

// Hydro 2 repeats one authored gate pattern on both sides of Sectors B and
// C/D: a New Tripwire sends TurnOn to two locked TweqLockedButton card
// readers, which in turn switch the translating door. The player may retain a
// matching card, but the tripwire must not present it for them: only a Frob on
// the reader legitimately clears PropLocked and enables later relays (#796).
//
// Negative-first on origin/main: after acquiring Hydro Card D without
// frobbing either reader, entering tripwire 1178 raised door 1181 by 3.6 units.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SECTOR_B_READER = 1120;
const SECTOR_B_READER_OTHER_SIDE = 1122;
const SECTOR_B_DOOR = 1127;
const SECTOR_CD_TRIPWIRE = 1178;
const SECTOR_CD_READER = 1152;
const SECTOR_CD_DOOR = 1181;
const HYDRO_CARD_B = -1495;
const HYDRO_CARD_D = -1496;

async function only(game: GameServer, objectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one hydro2 object ${objectId}, got ${JSON.stringify(found)}`,
  );
  return found[0];
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

async function position(game: GameServer, objectId: number): Promise<Vec3> {
  const entity = await only(game, objectId);
  return (await game.entities.detail(entity.id)).position;
}

async function closeDoor(game: GameServer, objectId: number): Promise<Vec3> {
  const door = await only(game, objectId);
  await game.entities.sendMessage(door.id, { type: "TurnOff" });
  await game.step({ frames: 240 });
  return (await game.entities.detail(door.id)).position;
}

test(
  "hydro2: keycard gates cannot activate before their locked readers are frobbed",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    await game.step({ frames: 5 });

    assert.equal((await game.player.inventory()).count, 0);

    const sectorBDoor = await only(game, SECTOR_B_DOOR);
    const sectorBBefore = sectorBDoor.position;
    const sectorBReader = await only(game, SECTOR_B_READER);
    await game.entities.sendMessage(sectorBReader.id, { type: "Frob" });
    await game.step({ frames: 240 });
    const sectorBAfter = (await game.entities.detail(sectorBDoor.id)).position;
    assert.ok(
      distance(sectorBBefore, sectorBAfter) < 0.01,
      `an empty-card reader frob must leave Sector B closed: ${JSON.stringify({ sectorBBefore, sectorBAfter })}`,
    );

    // Presenting the matching card is the legitimate transition: the reader
    // opens the door and clears its own authored lock.
    const cardB = await game.player.spawnItem(HYDRO_CARD_B);
    await game.entities.sendMessage(cardB.entity_id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal((await game.player.inventory()).count, 0);
    await game.entities.sendMessage(sectorBReader.id, { type: "Frob" });
    await game.step({ frames: 240 });
    const sectorBOpened = await position(game, SECTOR_B_DOOR);
    assert.ok(
      distance(sectorBBefore, sectorBOpened) > 3.5,
      `the matching Hydro B card must open Sector B: ${JSON.stringify({ sectorBBefore, sectorBOpened })}`,
    );

    // PropLocked is persisted game state. After save/load, a scripted TurnOn
    // may pass through the reader the player already unlocked.
    const sectorBClosed = await closeDoor(game, SECTOR_B_DOOR);
    assert.ok(distance(sectorBBefore, sectorBClosed) < 0.01);
    await game.save("hydro2-keycard-gates-e2e");
    await game.load("hydro2-keycard-gates-e2e");
    await game.step({ frames: 5 });
    const loadedReader = await only(game, SECTOR_B_READER);
    await game.entities.sendMessage(loadedReader.id, { type: "TurnOn" });
    await game.step({ frames: 240 });
    const sectorBRelayed = await position(game, SECTOR_B_DOOR);
    assert.ok(
      distance(sectorBBefore, sectorBRelayed) > 3.5,
      "an explicitly unlocked reader must keep relaying after save/load",
    );

    // The key itself is retained too: the opposite still-locked reader can be
    // legitimately unlocked without acquiring a second card entity.
    await closeDoor(game, SECTOR_B_DOOR);
    const otherReader = await only(game, SECTOR_B_READER_OTHER_SIDE);
    await game.entities.sendMessage(otherReader.id, { type: "Frob" });
    await game.step({ frames: 240 });
    assert.ok(
      distance(sectorBBefore, await position(game, SECTOR_B_DOOR)) > 3.5,
      "the retained Hydro B card must unlock the opposite reader",
    );

    // Acquire the real Hydro D key state without pressing either reader. Key
    // cards are retained in QuestInfo and the pickup entity is destroyed, so
    // the ordinary inventory remains empty after this production script path.
    const card = await game.player.spawnItem(HYDRO_CARD_D);
    await game.entities.sendMessage(card.entity_id, { type: "Frob" });
    await game.step({ frames: 5 });
    assert.equal((await game.player.inventory()).count, 0);

    const sectorCdDoor = await only(game, SECTOR_CD_DOOR);
    const sectorCdBefore = sectorCdDoor.position;
    const tripwire = await only(game, SECTOR_CD_TRIPWIRE);
    const [x, y, z] = tripwire.position;
    await game.player.teleport({ x, y: y + 0.5, z });
    await game.step({ frames: 240 });
    const sectorCdAfter = (await game.entities.detail(sectorCdDoor.id)).position;
    assert.ok(
      distance(sectorCdBefore, sectorCdAfter) < 0.01,
      `possessing a card must not let a tripwire bypass still-locked readers: ${JSON.stringify({ sectorCdBefore, sectorCdAfter })}`,
    );

    // Once the player presents the card, normal reader activation and later
    // tripwire relay behavior remain valid.
    const sectorCdReader = await only(game, SECTOR_CD_READER);
    await game.entities.sendMessage(sectorCdReader.id, { type: "Frob" });
    await game.step({ frames: 240 });
    assert.ok(
      distance(sectorCdBefore, await position(game, SECTOR_CD_DOOR)) > 3.5,
      "presenting Hydro D must open the C/D gate",
    );
    await closeDoor(game, SECTOR_CD_DOOR);
    await game.player.teleport({ x: 30.4, y: 0.6, z: -7.2 });
    await game.step({ frames: 10 });
    await game.player.teleport({ x, y: y + 0.5, z });
    await game.step({ frames: 240 });
    assert.ok(
      distance(sectorCdBefore, await position(game, SECTOR_CD_DOOR)) > 3.5,
      "the tripwire must relay through a legitimately unlocked reader",
    );
  },
);
