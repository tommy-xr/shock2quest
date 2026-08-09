import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { join } from "node:path";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Position } from "../src/types.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
// Stateful retail save supplied locally by the play-through campaign. The
// binary is intentionally not committed or uploaded; pass its bare logical
// name after copying it under DARK_ASSET_PATH/saves.
const arrivalSave = process.env.SHOCK2_REC_ARRIVAL_SAVE;

const ELEVATOR_BUTTON = 545;
const ARRIVAL_TRIPWIRE = 752;
const ARRIVAL_DOORS = [1922, 1923] as const;
const ARRIVAL_ONCE_ROUTER = 29;

async function only(game: GameServer, objectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one rec1 object ${objectId}, got ${JSON.stringify(found)}`,
  );
  return found[0];
}

async function walkTo(game: GameServer, target: Position): Promise<void> {
  let position = await game.player.position();
  for (let attempt = 0; attempt < 3; attempt++) {
    await game.player.moveTo(target);
    await game.step({ frames: 30 });
    position = await game.player.position();
    if (Math.hypot(position.x - target.x, position.z - target.z) < 0.4) break;
  }
  assert.ok(
    Math.hypot(position.x - target.x, position.z - target.z) < 0.4,
    `bounded movement should reach ${JSON.stringify(target)}, got ${JSON.stringify(position)}`,
  );
}

function playedEmails(saveName: string): string[] {
  const dataRoot = process.env.DARK_ASSET_PATH;
  assert.ok(dataRoot, "DARK_ASSET_PATH is required for the stateful arrival-room E2E");
  const save = JSON.parse(
    readFileSync(join(dataRoot, "saves", `${saveName}.sav`), "utf8"),
  ) as { global_data: { quest_info: { played_emails: string[] } } };
  return save.global_data.quest_info.played_emails;
}

test(
  "rec1: entering the arrival room activates the authored objective chain (#859)",
  { skip: !e2eEnabled || arrivalSave === undefined, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "rec1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8198),
    });
    assert.ok(arrivalSave);
    assert.equal((await game.load(arrivalSave)).success, true);
    await game.step({ frames: 1 });

    assert.equal(await game.quests.get("note_4_4"), "unknown");
    assert.equal(await game.quests.get("note_5_7"), "unknown");
    assert.equal(await game.quests.get("reprogram"), "incomplete");
    assert.equal(await game.quests.get("shodanroom"), "incomplete");

    // Leave the real main-elevator car through its four authored doors. This
    // is a production aim + squeeze interaction, not an injected Frob.
    const elevatorButton = await only(game, ELEVATOR_BUTTON);
    await game.player.aimAt(elevatorButton, {
      hitbox: "center",
      visibility: "required",
    });
    await game.input.set("right_hand.squeeze_value", 1);
    await game.step({ frames: 2 });
    await game.input.set("right_hand.squeeze_value", 0);
    await game.step({ frames: 60 });

    for (const target of [
      { x: -4.5, y: -3.956, z: -128.9 },
      { x: -2.5, y: -3.956, z: -128.9 },
      { x: -1.5, y: -3.956, z: -131.0 },
      { x: -2.0, y: -3.956, z: -134.0 },
      { x: -6.5, y: -3.956, z: -137.75 },
    ]) {
      await walkTo(game, target);
    }

    // Enter Tripwire 752 by bounded locomotion. It opens the real 1922/1923
    // door pair, which proves the route rather than teleporting into the room.
    await walkTo(game, { x: -8.47, y: -3.956, z: -137.87 });
    await game.step({ frames: 180 });
    const afterDoors = await Promise.all(
      ARRIVAL_DOORS.map(async (objectId) => (await only(game, objectId)).position),
    );
    assert.ok(
      Math.abs(afterDoors[0][2] - afterDoors[1][2]) > 5,
      `arrival doors should be physically spread open through Tripwire ${ARRIVAL_TRIPWIRE}, ` +
        `got z=${afterDoors[0][2]} / ${afterDoors[1][2]}`,
    );

    await walkTo(game, { x: -9.5, y: -3.956, z: -137.8 });
    await walkTo(game, { x: -11.75, y: -3.956, z: -137.6 });
    await game.step({ frames: 180 });

    assert.equal(await game.quests.get("note_4_4"), "complete");
    assert.equal(await game.quests.get("note_5_7"), "incomplete");
    assert.equal(
      (await game.entities.byTemplate(ARRIVAL_ONCE_ROUTER)).length,
      0,
      "the room entry should consume its authored OnceRouter after firing",
    );

    const firstEntrySave = `rec_arrival_first_entry_${Date.now()}`;
    assert.equal((await game.save(firstEntrySave)).success, true);
    const firstEmails = playedEmails(firstEntrySave);
    assert.equal(firstEmails.filter((email) => email === "EM0509").length, 1);
    assert.equal(firstEmails.includes("EM0511"), false);

    // Walk fully out of the ROOM_DB volume, then back in. OnceRouter 29 and
    // Buffy/TrapSlayer make the authored objective/email chain one-shot.
    for (const target of [
      { x: -9.5, y: -3.956, z: -137.8 },
      { x: -8.47, y: -3.956, z: -137.87 },
      { x: -6.5, y: -3.956, z: -137.75 },
    ]) {
      await walkTo(game, target);
    }
    await game.step({ frames: 60 });
    for (const target of [
      { x: -8.47, y: -3.956, z: -137.87 },
      { x: -9.5, y: -3.956, z: -137.8 },
      { x: -11.75, y: -3.956, z: -137.6 },
    ]) {
      await walkTo(game, target);
    }
    await game.step({ frames: 180 });

    assert.equal(await game.quests.get("note_4_4"), "complete");
    assert.equal(await game.quests.get("note_5_7"), "incomplete");
    const secondEntrySave = `rec_arrival_second_entry_${Date.now()}`;
    assert.equal((await game.save(secondEntrySave)).success, true);
    const secondEmails = playedEmails(secondEntrySave);
    assert.equal(secondEmails.filter((email) => email === "EM0509").length, 1);
    assert.equal(secondEmails.includes("EM0511"), false);

    const dataRoot = process.env.DARK_ASSET_PATH!;
    rmSync(join(dataRoot, "saves", `${firstEntrySave}.sav`));
    rmSync(join(dataRoot, "saves", `${secondEntrySave}.sav`));
  },
);
