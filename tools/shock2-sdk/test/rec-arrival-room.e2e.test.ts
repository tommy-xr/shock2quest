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
const commandArrivalSave = process.env.SHOCK2_COMMAND_ARRIVAL_SAVE;

const ELEVATOR_BUTTON = 545;
const ARRIVAL_TRIPWIRE = 752;
const ARRIVAL_DOORS = [1922, 1923] as const;
const ARRIVAL_ONCE_ROUTER = 29;
const ARRIVAL_ROUTER = 26;
const ARRIVAL_EMAIL_TRAP = 171;
const COMMAND_RETURN_BUTTON = 422;
const COMMAND_ELEVATOR_BUTTON = 512;

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
  const arrivalTolerance = 0.8;
  let position = await game.player.position();
  for (let attempt = 0; attempt < 3; attempt++) {
    await game.player.moveTo(target);
    await game.step({ frames: 30 });
    position = await game.player.position();
    if (
      Math.hypot(position.x - target.x, position.z - target.z) < arrivalTolerance
    )
      break;
  }
  if (
    Math.hypot(position.x - target.x, position.z - target.z) < arrivalTolerance
  )
    return;
  const nearby = (await game.entities.list({ limit: 12 })).entities.map(
    ({ name, template_id, position }) => ({ name, template_id, position }),
  );
  assert.fail(
    `bounded movement should reach ${JSON.stringify(target)}, got ${JSON.stringify(position)}; ` +
      `nearby=${JSON.stringify(nearby)}`,
  );
}

async function squeezeButton(
  game: GameServer,
  objectId: number,
): Promise<void> {
  const button = await only(game, objectId);
  await game.player.aimAt(button, {
    hitbox: "center",
    visibility: "required",
  });
  await game.input.set("right_hand.squeeze_value", 1);
  await game.step({ frames: 2 });
  await game.input.set("right_hand.squeeze_value", 0);
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
    await squeezeButton(game, ELEVATOR_BUTTON);
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

test(
  "rec1: a late Command return does not regress a completed objective (#859)",
  { skip: !e2eEnabled || commandArrivalSave === undefined, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8198),
    });
    assert.ok(commandArrivalSave);
    assert.equal((await game.load(commandArrivalSave)).success, true);
    await game.step({ frames: 1 });

    assert.equal((await game.info()).mission, "command1.mis");
    assert.equal(await game.quests.get("note_4_4"), "unknown");
    assert.equal(await game.quests.get("note_5_7"), "complete");

    // Operate Command's real return-elevator button. It synchronously runs
    // SimpleLevelChangeButton's authored rec1 / loc 65 transition.
    await squeezeButton(game, COMMAND_RETURN_BUTTON);
    await game.step({ frames: 2 });
    assert.equal((await game.info()).mission, "rec1.mis");

    // Open the real Recreation command-elevator doors, then traverse the
    // shipped navigation corridor to the arrival vestibule by locomotion.
    await squeezeButton(game, COMMAND_ELEVATOR_BUTTON);
    await game.step({ frames: 60 });
    for (const target of [
      { x: 35.8, y: -3.956, z: -113.8 },
      { x: 32.9, y: -3.956, z: -113.8 },
      { x: 31.1, y: -3.956, z: -112.0 },
      { x: 28.1, y: -3.956, z: -115.0 },
      { x: 24.4, y: -3.956, z: -114.8 },
      { x: 20.5, y: -3.956, z: -114.8 },
      { x: 20.2, y: -3.956, z: -115.8 },
      { x: 16.1, y: -3.956, z: -116.4 },
      { x: 12.1, y: -3.956, z: -117.05 },
      { x: 7.7, y: -3.956, z: -119.45 },
      { x: 7.7, y: -3.956, z: -125.5 },
      { x: 7.7, y: -3.956, z: -131.7 },
      { x: 6.0, y: -3.956, z: -135.4 },
      { x: 4.2, y: -3.956, z: -136.6 },
      { x: 3.85, y: -3.956, z: -136.98 },
      { x: 3.72, y: -3.956, z: -137.02 },
      { x: 3.6, y: -3.956, z: -137.7 },
      { x: -2.3, y: -3.956, z: -138.2 },
      { x: -6.5, y: -3.956, z: -137.75 },
      { x: -8.47, y: -3.956, z: -137.87 },
    ]) {
      await walkTo(game, target);
    }
    await game.step({ frames: 180 });
    await walkTo(game, { x: -9.5, y: -3.956, z: -137.8 });
    await walkTo(game, { x: -11.75, y: -3.956, z: -137.6 });
    await game.step({ frames: 180 });

    assert.equal(await game.quests.get("note_4_4"), "complete");
    assert.equal(
      await game.quests.get("note_5_7"),
      "complete",
      "late arrival must not overwrite the already-completed objective",
    );
    for (const consumed of [
      ARRIVAL_ROUTER,
      ARRIVAL_ONCE_ROUTER,
      ARRIVAL_EMAIL_TRAP,
    ]) {
      assert.equal(
        (await game.entities.byTemplate(consumed)).length,
        0,
        `authored one-shot object ${consumed} should be consumed`,
      );
    }

    const lateEntrySave = `rec_arrival_late_command_entry_${Date.now()}`;
    assert.equal((await game.save(lateEntrySave)).success, true);
    const emails = playedEmails(lateEntrySave);
    assert.equal(emails.filter((email) => email === "EM0509").length, 1);
    assert.equal(emails.includes("EM0511"), false);
    rmSync(join(process.env.DARK_ASSET_PATH!, "saves", `${lateEntrySave}.sav`));
  },
);
