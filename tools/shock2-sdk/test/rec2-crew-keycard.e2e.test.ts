import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { GameServer, findRepoRoot } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/types.js";
import { clickUiElement } from "./helpers/ui.js";

// 25th Anniversary campaign regression for #583. The copyrighted frontier
// save remains outside the repository; when present it reproduces the exact
// Recreation route immediately before the crew access card trip.
const FIXTURE_SAVE =
  "campaign_recreation_25th_iter10_rec2_beyond_midwife_before_card_trip_20260808";
const FIXTURE_SHA256 =
  "d9e77ae096e8711c8d3d6d017a6e43a8ea91eff38e30d95b01e7ac00eb6d859d";
const CORPSE = 419;
const CREW_CARD = 996;
const CREW_SLOT = 293;
const CREW_DOORS = [1580, 1582] as const;

function savePath(saveName: string): string | undefined {
  const repoRoot = findRepoRoot(process.cwd()) ?? process.cwd();
  const roots = [
    process.env.DARK_ASSET_PATH,
    join(repoRoot, "Data"),
    join(repoRoot, "..", "Data"),
  ].filter((root): root is string => Boolean(root));
  return roots
    .map((root) => join(root, "saves", `${saveName}.sav`))
    .find(existsSync);
}

const fixturePath = savePath(FIXTURE_SAVE);
const e2eEnabled = process.env.SHOCK2_E2E === "1" && Boolean(fixturePath);
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8583);

function only(matches: EntitySummary[], label: string): EntitySummary {
  assert.equal(matches.length, 1, `expected one ${label}, got ${matches.length}`);
  return matches[0];
}

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

// Registering a key source consumes the object: access lives on the player's
// keyring, not in the backpack. So the card must be gone once it is taken.
async function assertCardRegisteredAndConsumed(game: GameServer): Promise<void> {
  // `/v1/entities` enumerates contained items too (that is how the card is
  // found inside corpse 419 before the take), so an empty result also proves
  // the card was not stashed in the backpack.
  assert.equal(
    (await game.entities.byTemplate(CREW_CARD)).length,
    0,
    "a registered crew card must no longer exist as an object, in world or backpack",
  );
}

test(
  "Rec2 corpse loot registers the crew card and grants quest reward and persistent Rec1 access",
  { skip: !e2eEnabled, timeout: 600_000 },
  async (t) => {
    assert.ok(fixturePath);
    assert.equal(
      createHash("sha256").update(readFileSync(fixturePath)).digest("hex"),
      FIXTURE_SHA256,
      "the regression must use the reviewed campaign frontier",
    );

    const persistedSave = `rec2_crew_keycard_e2e_${Date.now()}`;
    t.after(() => {
      const path = savePath(persistedSave);
      if (path) rmSync(path, { force: true });
    });

    let expectedModules: number;

    // Session 1: resume the exact Rec2 frontier and perform the real player
    // interaction: bounded movement, corpse surface aim/squeeze, then the
    // authentic Container MFD card button through pointer input.
    {
      await using game = await GameServer.launch({
        mission: "rec2.mis",
        port: basePort,
        echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      });
      assert.equal((await game.load(FIXTURE_SAVE)).success, true);
      await game.step({ frames: 5 });

      assert.equal(await game.quests.get("crewcahd"), "unknown");
      const modulesBefore = (await game.info()).player.stats?.cyber_modules;
      assert.notEqual(modulesBefore, undefined);
      const corpse = only(await game.entities.byTemplate(CORPSE), "Rec2 corpse 419");
      const card = only(await game.entities.byTemplate(CREW_CARD), "Rec2 card 996");

      await game.player.moveTo({ x: 96.0, y: -8.756, z: -54.0 });
      await game.step({ frames: 8 });
      await game.player.moveTo({ x: 97.7, y: -8.756, z: -54.0 });
      await game.step({ frames: 8 });
      const aim = await game.player.aimAt(corpse.id, {
        hitbox: "center",
        visibility: "required",
      });
      assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
      await game.input.set("right_hand.squeeze_value", 1);
      await game.step({ frames: 2 });
      await game.input.set("right_hand.squeeze_value", 0);
      await game.step({ frames: 5 });

      const panel = (await game.ui.state()).active_panel;
      assert.equal(panel?.entity_id, corpse.id, "corpse 419 must open its real loot MFD");
      const cardButton = panel.elements.find(
        (element) => element.kind === "button" && element.entity_id === card.id,
      );
      assert.ok(cardButton, "corpse 419 must expose contained card 996");
      await clickUiElement(game, cardButton);
      await game.step({ frames: 30 });

      await assertCardRegisteredAndConsumed(game);
      assert.equal(
        await game.quests.get("crewcahd"),
        "incomplete",
        "FrobQB must set the authored crew-access quest bit",
      );
      expectedModules = (modulesBefore as number) + 20;
      assert.equal(
        (await game.info()).player.stats?.cyber_modules,
        expectedModules,
        "the crewcahd trigger must award exactly 20 cyber modules",
      );
      await game.step({ frames: 120 });
      assert.equal(
        (await game.info()).player.stats?.cyber_modules,
        expectedModules,
        "the one-shot experience trap must not pay twice",
      );
      assert.equal((await game.save(persistedSave)).success, true);
    }

    // Session 2: a fresh process reloads the result, then visits Rec1. The
    // level warp and nearby placement are setup only; the card slot itself is
    // aimed and squeezed through normal production input.
    {
      await using game = await GameServer.launch({
        mission: "rec1.mis",
        port: basePort + 1,
        echoLogs: process.env.SHOCK2_ECHO_LOGS === "1",
      });
      assert.equal((await game.load(persistedSave)).success, true);
      await game.step({ frames: 5 });
      assert.equal((await game.info()).mission, "rec2.mis");
      await assertCardRegisteredAndConsumed(game);
      assert.equal(await game.quests.get("crewcahd"), "incomplete");
      assert.equal((await game.info()).player.stats?.cyber_modules, expectedModules);

      assert.equal((await game.transitionLevel("rec1.mis")).success, true);
      await game.step({ frames: 10 });
      await assertCardRegisteredAndConsumed(game);

      const slot = only(await game.entities.byTemplate(CREW_SLOT), "Rec1 crew card slot 293");
      const doors = await Promise.all(
        CREW_DOORS.map(async (templateId) =>
          only(await game.entities.byTemplate(templateId), `Rec1 crew door ${templateId}`),
        ),
      );
      const before = await Promise.all(
        doors.map(async (door) => (await game.entities.detail(door.id)).position),
      );
      const soundSequence = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;

      await game.player.teleport({ x: 12.05, y: -3.956, z: -142.5 });
      await game.step({ frames: 3 });
      const aim = await game.player.aimAt(slot.id, {
        hitbox: "center",
        visibility: "required",
      });
      assert.equal(aim.target_confirmed, true, JSON.stringify(aim));
      await game.input.set("right_hand.squeeze_value", 1);
      await game.step({ frames: 2 });
      await game.input.set("right_hand.squeeze_value", 0);
      await game.step({ frames: 180 });

      const after = await Promise.all(
        doors.map(async (door) => (await game.entities.detail(door.id)).position),
      );
      assert.ok(
        after.some((position, index) => distance(position, before[index]) > 0.5),
        `region-32 access must open the linked Rec1 doors: before=${JSON.stringify(before)}, after=${JSON.stringify(after)}`,
      );
      const played = (await game.audio.recent()).sounds
        .filter((sound) => sound.sequence > soundSequence)
        .map((sound) => sound.sample.toLowerCase());
      assert.ok(
        !played.includes("hackfail"),
        `the acquired crew card must not be refused: ${JSON.stringify(played)}`,
      );
    }
  },
);
