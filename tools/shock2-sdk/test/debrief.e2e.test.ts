import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { isCutsceneScene } from "./helpers/cutscenes.js";
import { DEBRIEF_CONTINUE, dismissDebrief } from "./helpers/debrief.js";
import { vrClickCanvasPoint } from "./helpers/frontend-menu.js";

// The character-creation debrief: the page the original shows as a training
// tour ends ("Your stint aboard the UNN Gallo is finished... You've gained +2
// Strength."), on its own authored screen (DEBRIEF.PCX + DEBRIEFR.BIN) rather
// than as an overlay, with a Continue button that starts the departure.
//
// The tour markers in station.mis carry no text - only P$CharGenRo, the tour
// index - so the string is selected the way the original does: (career, year,
// tour) picks a Mission1..Mission27 key in res/strings/CHARGEN.STR, which the
// reward table already records.
//
// Negative-first: on the base revision nothing implements the debrief at all
// (`TourReward::text_key` is read by nobody), so the tour trigger swaps
// straight to the departure and every assertion here fails - there is no
// "debrief" scene to land on.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8100);

/** The debrief only lands with a career chosen; chargen sets this bit. */
const CAREER_BIT = "career_marine";
/** Marine year 1 tour 0 = CHARGEN.STR "Mission1". */
const EXPECTED_PAGE = "UNN Gallo";
const EXPECTED_GRANT = "+2 Strength";
/**
 * Complete a training tour: pick a career, then fire a ChooseMission marker
 * exactly as the tour tripwire does.
 *
 * The three markers differ only in `P$CharGenRo` (the tour index) and their
 * authored position, and runtime entity ids are not stable, so the tour is
 * pinned by position: tour 0 is the marker at the greatest z (-22.6, -5.6,
 * 11.2 in the mission file). Both presentations must complete the SAME tour,
 * or their screenshots show different pages and prove nothing about layout.
 */
async function completeTourZero(game: GameServer): Promise<void> {
  await game.quests.set(CAREER_BIT, "complete");
  const markers = (await game.transitions()).transitions.filter((t) =>
    t.dest_level.toLowerCase().includes("medsci1"),
  );
  assert.equal(markers.length, 3, "station has three ChooseMission tour markers");
  const tourZero = markers.reduce((a, b) => (a.position[2] >= b.position[2] ? a : b));
  // TurnOn is a valid debug message; the SDK's typed union predates it.
  await game.entities.sendMessage(tourZero.entity_id, {
    type: "TurnOn",
  } as unknown as Parameters<typeof game.entities.sendMessage>[1]);
  await game.step({ frames: 30 });
}

test(
  "flat: finishing a training tour opens the debrief page, and Continue departs",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
      port: basePort,
    });
    await game.step({ frames: 30 });

    assert.equal(
      (await game.ui.state()).debrief_text,
      null,
      "no debrief may show before a tour is completed",
    );

    await completeTourZero(game);

    // The tour hands the screen to the debrief, before the departure - the
    // level change waits behind the page.
    assert.equal((await game.info()).mission, "debrief");
    // The page really is the tour's own CHARGEN.STR entry, not just "some text
    // drew": a wrong key would still emit glyphs.
    const page = (await game.ui.state()).debrief_text ?? "";
    assert.match(page, new RegExp(EXPECTED_PAGE), `wrong debrief page: ${page}`);
    assert.match(page, new RegExp(EXPECTED_GRANT.replace("+", "\\+")));

    // It is a page, not a timed overlay: it waits for the player.
    await game.step({ frames: 20 * 60 });
    assert.equal((await game.info()).mission, "debrief", "the page must wait to be dismissed");

    assert.ok(await dismissDebrief(game), "Continue should be clickable on the page");
    const after = (await game.info()).mission;
    assert.ok(
      isCutsceneScene(after) || after.toLowerCase().includes("station"),
      `Continue should start the departure, got ${after}`,
    );
    assert.equal((await game.ui.state()).debrief_text, null, "and leave the page behind");
  },
);

test(
  "vr: the same tour's page presents on the world panel, and its Continue works",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
      port: basePort + 1,
      debugFlags: ["--vr"],
    });
    await game.step({ frames: 30 });

    await completeTourZero(game);

    assert.equal((await game.info()).mission, "debrief");
    const page = (await game.ui.state()).debrief_text ?? "";
    assert.match(page, new RegExp(EXPECTED_PAGE), `wrong debrief page: ${page}`);

    // The screen is on the shared frontend panel in front of the head, not left
    // at the world origin.
    const objects = (await game.scene.objects()).objects;
    const depths = objects.filter((o) => o.source === null).map((o) => -o.position[0]);
    assert.ok(depths.length > 0, "the VR presentation must draw the page on its panel");
    assert.ok(
      Math.min(...depths) > 0.5 && Math.max(...depths) < 6,
      `the panel should hang in front of the head, got ${JSON.stringify(depths)}`,
    );

    // The same canvas rect drives the click in VR, through the controller ray.
    await vrClickCanvasPoint(game, DEBRIEF_CONTINUE);
    const after = (await game.info()).mission;
    assert.ok(
      isCutsceneScene(after) || after.toLowerCase().includes("station"),
      `Continue should start the departure in VR too, got ${after}`,
    );
  },
);
