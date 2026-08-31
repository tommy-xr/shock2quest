import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { stepPastCutscenes } from "./helpers/cutscenes.js";
import { dismissDebrief } from "./helpers/debrief.js";

// End-to-end test for the HONEST character-creation chain the player walks at
// game start: earth.mis (pick a career) -> station.mis three training tours ->
// deploy to MedSci1.mis. Requires game assets in Data/ and compiles the runtime
// on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Unlike the sibling `station-career*` tests - which inject `TurnOn` straight
// into a marker and warp via `transitionLevel()` - this test drives the REAL
// tripwire wiring: it teleports the player ONTO each career-door / tour tripwire
// volume so the actual `TrapNewTripwire` -> SwitchLink -> `ChooseService` /
// `ChooseMission` script chain fires (teleporting into a trigger volume fires
// the same SensorBeginIntersect ENTER trigger as walking in - honest walking of
// earth.mis was proven manually in the 2026-07-10 verification session). It is a
// characterization test: it must PASS on `main` and encodes the flow as a
// permanent regression net.
//
// #453 (LANDED): training tours now grant stats/skills per the (career, year,
// tour) reward table mirroring CHARGEN.STR. This test's Marine chain fires tour
// marker 125 (P$CharGenRo = tour 0) once per year, so it applies the tour-0
// grant of each Marine year: Y1 Mission1 (+2 STR), Y2 Mission4 (+1 Energy
// Weapons, +1 Cyber Affinity), Y3 Mission7 (+1 Maintenance). We assert each
// grant as it lands and the cumulative sheet surviving the deploy to MedSci1
// (and a save/load round-trip mid-flow). These assertions read
// `info.player.stats`, which did not exist before #453 - the pre-#453 runtime
// reports no stats and applies no grants, so they fail on `main` (negative).
//
// #454 (FIXED): post-career station arrival spawns at the designed
// recruit-deck spot (81.78,-3.6,16.54) - the StartLoc marker's own position -
// rather than the world origin. We assert the designed spot (within ~3 units,
// allowing physics settle).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Stable mission-data world positions (dark_query). These are the TRIPWIRE
// volume centers - the entities the player walks through - not the markers they
// switch on. Runtime entity ids are not stable across runs, but mission-file
// positions are, so position is the durable handle (same approach the sibling
// career test uses).
//
// earth.mis career-door tripwires, keyed by the career they enlist you in. Note
// the Navy door's tripwire (217) switches on a marker literally named
// "SendToMarines" yet carrying P$Service=1 (Navy): a retail data misnomer - the
// engine follows P$Service, not the name. Asserting Navy here guards that.
const CAREER_DOORS = {
  marine: { trip: [-2.4990878, 24.0, 68.16256] as Vec3, bit: "career_marine", maxHp: 45, maxPsi: 20 },
  navy: { trip: [0.5085414, 24.0, 84.064316] as Vec3, bit: "career_navy", maxHp: 35, maxPsi: 35 },
  osa: { trip: [16.857706, 24.0, 80.74739] as Vec3, bit: "career_osa", maxHp: 30, maxPsi: 60 },
} as const;

// station.mis tour tripwire (741) - switches on tour marker 125 (ChooseMission).
// Its volume is isolated on x=-22.6 (the post-tour respawn at ~81.78 is nowhere
// near it), so re-teleporting onto the same center each tour is a fresh ENTER.
// Every tour's ChooseMission TurnOn just advances the training-year counter, so
// one real tour tripwire exercises the whole tripwire->marker->bit path.
const TOUR_TRIP: Vec3 = [-22.6, -5.6, 8.0];

// #454: the designed post-career recruit-deck spawn (station.mis marker obj 132,
// PropStartLoc 2501, whose own PropPosition is used now that it has no
// LandingPoint link). Allow ~3 units of slack for the physics settle after spawn.
const CAREER_SPAWN_DESIGNED: Vec3 = [81.78026, -3.6, 16.539234];
const CAREER_SPAWN_TOLERANCE = 3.0;

// START_01's YEAR-1 chain teleports the authored recruiter (mission object
// 881, exposed as the stable template_id) from its below-map storage position
// into this exhibit. Checking that exact object guards the destination setup;
// merely reaching station.mis or its exit tripwire does not.
const MARINE_YEAR_ONE_RECRUITER_TEMPLATE = 881;
const MARINE_YEAR_ONE_RECRUITER_POSITION: Vec3 = [15.6, -3.2, 10.4];
const STAGED_ENTITY_TOLERANCE = 1.0;

async function assertStagedEntity(
  game: GameServer,
  filter: string,
  templateId: number,
  expected: Vec3,
  label: string,
): Promise<void> {
  const entity = (await game.entities.list({ filter, limit: 20 })).entities.find(
    (candidate) => candidate.template_id === templateId,
  );
  assert.ok(entity, `${label}: expected stable template ${templateId}`);
  assert.ok(entity.position, `${label}: entity should expose a world position`);
  const delta = Math.hypot(
    entity.position[0] - expected[0],
    entity.position[1] - expected[1],
    entity.position[2] - expected[2],
  );
  assert.ok(
    delta < STAGED_ENTITY_TOLERANCE,
    `${label}: expected template ${templateId} near ${JSON.stringify(expected)}, got ${JSON.stringify(
      entity.position,
    )} (delta ${delta.toFixed(2)}u)`,
  );
}

async function teleportTo(game: GameServer, [x, y, z]: Vec3): Promise<void> {
  await game.player.teleport({ x, y, z });
}

/**
 * Trip a chargen tripwire and arrive at its destination. Each of these
 * transitions plays its authored movie first (see campaign-cutscenes.e2e.test.ts
 * for the mapping), so the destination is only on screen once those finish;
 * this test is about what survives the chain, not about the movies.
 */
async function tripAndArrive(game: GameServer, at: Vec3, frames: number): Promise<string[]> {
  await teleportTo(game, at);
  await game.step({ frames });
  // A tour ends on its debrief page, which waits for the player before the
  // departure runs; a career door has none to dismiss.
  await dismissDebrief(game);
  return stepPastCutscenes(game);
}

/** The set of `training_year_N` bits currently COMPLETE, sorted ascending. */
async function completeTrainingYears(game: GameServer): Promise<string[]> {
  const { quests } = await game.quests.list();
  return quests
    .filter((q) => q.name.startsWith("training_year_") && q.value === "complete")
    .map((q) => q.name)
    .sort();
}

test(
  "station flow: earth career door -> 3 training tours -> deploy to MedSci1 (Marine chain)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "earth.mis",
    });
    await game.step({ frames: 5 });
    await game.screenshot("station-flow-earth.png");

    // Step 1: enlist as a Marine by walking through the Marine career door.
    // Teleport ONTO its tripwire volume (equivalent to walking in: fires the same
    // ENTER trigger) and let the real TrapNewTripwire -> ChooseService chain run.
    await tripAndArrive(game, CAREER_DOORS.marine.trip, 15);

    let info = await game.info();
    assert.equal(
      info.mission.toLowerCase(),
      "station.mis",
      "entering the Marine career door should transition to the recruit station",
    );
    // Career persisted, mutually exclusive with the other branches.
    assert.equal(await game.quests.get("career_marine"), "complete", "Marine career bit should be set");
    assert.equal(await game.quests.get("career_navy"), "unknown", "Navy bit should be clear");
    assert.equal(await game.quests.get("career_osa"), "unknown", "OSA bit should be clear");
    // Marine loadout applied on the station load.
    assert.equal(info.player.hit_points, 45, "first Marine selection starts at full career HP");
    assert.equal(info.player.max_hit_points, 45, "Marine deploys with 45 max HP");
    assert.equal(info.player.psi_points, 20, "first Marine selection starts at full career psi");
    assert.equal(info.player.max_psi_points, 20, "Marine deploys with 20 max psi");

    // #453: the persistent character sheet exists and starts at baseline (no
    // tours completed yet). On `main` this field is absent, so `base` is
    // null/undefined and every assertion below fails (the negative).
    const base = info.player.stats;
    assert.ok(base, "#453: player.stats should be present at station arrival");
    assert.deepEqual(
      base.granted_years,
      [],
      "#453: no training-year rewards applied before any tour",
    );
    const baseStr = base.strength;
    const baseCyb = base.cyber_affinity;
    // #454 (fixed): post-career arrival lands at the designed recruit-deck spot
    // (the StartLoc marker's own position), not the world origin.
    const arrival = info.player.position;
    const spawnDelta = Math.hypot(
      arrival[0] - CAREER_SPAWN_DESIGNED[0],
      arrival[1] - CAREER_SPAWN_DESIGNED[1],
      arrival[2] - CAREER_SPAWN_DESIGNED[2],
    );
    assert.ok(
      spawnDelta < CAREER_SPAWN_TOLERANCE,
      `#454: post-career arrival should be near the designed spot ${JSON.stringify(
        CAREER_SPAWN_DESIGNED,
      )} (within ${CAREER_SPAWN_TOLERANCE}u), got ${JSON.stringify(
        arrival,
      )} (delta ${spawnDelta.toFixed(2)}u)`,
    );

    // #497 (negative-first): loading Station must activate START_01 after the
    // destination scripts initialize. Before the fix, ChooseService passed an
    // empty entities_to_trigger list and this recruiter stayed below the map at
    // y=-26 even though the test could still teleport to the exit tripwire.
    await assertStagedEntity(
      game,
      "MaleRec",
      MARINE_YEAR_ONE_RECRUITER_TEMPLATE,
      MARINE_YEAR_ONE_RECRUITER_POSITION,
      "#497 START_01 recruiter",
    );

    // Step 2: tours 1 and 2 each set exactly the next training_year bit, loop
    // back to station.mis, and grant the tour-0 reward for that Marine year
    // (#453). hp/psi are unchanged by these grants (they touch STR/skills), so
    // the loadout stays 45/20 - the observable change is now in player.stats.
    await tripAndArrive(game, TOUR_TRIP, 20);
    info = await game.info();
    assert.equal(info.mission.toLowerCase(), "station.mis", "tour 1 (year < 4) should loop back to station.mis");
    assert.deepEqual(
      await completeTrainingYears(game),
      ["training_year_2"],
      "tour 1 should set exactly training_year_2",
    );
    // #453 Marine Y1 T0 (Mission1): +2 Strength.
    assert.ok(info.player.stats, "player.stats present after tour 1");
    assert.equal(info.player.stats.strength, baseStr + 2, "#453: tour 1 grants +2 Strength (Mission1)");
    assert.deepEqual(info.player.stats.granted_years, [1], "tour 1 records year 1 granted");
    assert.equal(info.player.max_hit_points, 45, "tour 1 leaves max HP unchanged (grant is STR)");
    assert.equal(info.player.max_psi_points, 20, "tour 1 leaves max psi unchanged");
    await assertStagedEntity(
      game,
      "Marines 4",
      360,
      [0.012, -1.6, 8.0],
      "#497 START_02 Marine choice",
    );

    await tripAndArrive(game, TOUR_TRIP, 20);
    info = await game.info();
    assert.equal(info.mission.toLowerCase(), "station.mis", "tour 2 (year < 4) should loop back to station.mis");
    assert.deepEqual(
      await completeTrainingYears(game),
      ["training_year_2", "training_year_3"],
      "tour 2 should set training_year_2 + training_year_3",
    );
    // #453 Marine Y2 T0 (Mission4): +1 Energy Weapons, +1 Cyber Affinity.
    assert.ok(info.player.stats, "player.stats present after tour 2");
    assert.equal(info.player.stats.strength, baseStr + 2, "STR from tour 1 carries into tour 2");
    assert.equal(info.player.stats.skills.energy_weapons, 1, "#453: tour 2 grants +1 Energy Weapons (Mission4)");
    assert.equal(info.player.stats.cyber_affinity, baseCyb + 1, "#453: tour 2 grants +1 Cyber Affinity (Mission4)");
    assert.deepEqual(info.player.stats.granted_years, [1, 2], "tour 2 records years 1+2 granted");
    await assertStagedEntity(
      game,
      "Marines 7",
      369,
      [0.012, -1.6, 8.0],
      "#497 START_03 Marine choice",
    );

    // Save/load leg: persist mid-flow (after year 2) and reload in the same
    // runtime; the accumulated stats must survive the round-trip byte-for-byte.
    const statsBeforeSave = info.player.stats;
    const saveName = `station_flow_453_${Date.now()}`;
    await game.save(saveName);
    await game.load(saveName);
    await game.step({ frames: 5 });
    info = await game.info();
    assert.equal(info.mission.toLowerCase(), "station.mis", "load restores station.mis");
    assert.ok(info.player.stats, "player.stats present after load");
    assert.deepEqual(
      info.player.stats,
      statsBeforeSave,
      "#453: the character sheet survives save/load unchanged",
    );

    // Step 3: the third tour advances to year 4, grants the Marine Y3 T0 reward
    // (Mission7: +1 Maintenance), and deploys to MedSci1 - stats survive the
    // level transition.
    // The deploy is the one moment that plays two movies, shuttle then boarding.
    assert.deepEqual(
      await tripAndArrive(game, TOUR_TRIP, 20),
      ["shuttle3.avi", "cs2.avi"],
      "the deploy should play the last shuttle, then the boarding of the Von Braun",
    );
    info = await game.info();
    assert.equal(
      info.mission.toLowerCase(),
      "medsci1.mis",
      "the third tour (year 4) should deploy to MedSci1",
    );
    assert.deepEqual(
      await completeTrainingYears(game),
      ["training_year_2", "training_year_3", "training_year_4"],
      "all three training years should be complete after deploying",
    );
    // Career and loadout survive the whole chain.
    assert.equal(await game.quests.get("career_marine"), "complete", "Marine career should survive deploy");
    assert.equal(info.player.max_hit_points, 45, "Marine max HP should survive deploy");
    assert.equal(info.player.max_psi_points, 20, "Marine max psi should survive deploy");
    // #453: the cumulative Marine tour-0 sheet survives the deploy into MedSci1.
    assert.ok(info.player.stats, "player.stats present at MedSci1");
    assert.equal(info.player.stats.strength, baseStr + 2, "cumulative +2 STR at MedSci1 (Mission1)");
    assert.equal(info.player.stats.cyber_affinity, baseCyb + 1, "cumulative +1 Cyber Affinity at MedSci1 (Mission4)");
    assert.equal(info.player.stats.skills.energy_weapons, 1, "cumulative +1 Energy Weapons at MedSci1 (Mission4)");
    assert.equal(info.player.stats.skills.maintenance, 1, "#453: tour 3 grants +1 Maintenance (Mission7)");
    assert.deepEqual(info.player.stats.granted_years, [1, 2, 3], "all three years granted at MedSci1");
    assert.ok(
      info.player.position.every(Number.isFinite),
      `deploy position should be finite, got ${JSON.stringify(info.player.position)}`,
    );
  },
);

/**
 * Enlist via one career door from a fresh earth.mis launch and read the arrival
 * career bit + loadout at the recruit station. A fresh launch per branch keeps
 * the runs independent.
 */
async function enlist(
  branch: keyof typeof CAREER_DOORS,
): Promise<{
  bit: string;
  hitPoints: number | null;
  maxHp: number | null;
  psiPoints: number | null;
  maxPsi: number | null;
}> {
  const door = CAREER_DOORS[branch];
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 5 });

  await tripAndArrive(game, door.trip, 15);

  const info = await game.info();
  assert.equal(
    info.mission.toLowerCase(),
    "station.mis",
    `${branch}: entering the career door should transition to the recruit station`,
  );
  return {
    bit: await game.quests.get(door.bit),
    hitPoints: info.player.hit_points,
    maxHp: info.player.max_hit_points,
    psiPoints: info.player.psi_points,
    maxPsi: info.player.max_psi_points,
  };
}

test(
  "station flow: Navy and OSA doors map to the correct career bit and loadout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // Navy: the door whose tripwire switches the mislabeled "SendToMarines"
    // marker (P$Service=1). Correct engine behavior yields the Navy career.
    const navy = await enlist("navy");
    assert.equal(navy.bit, "complete", "Navy door should set career_navy (despite the 'SendToMarines' misnomer)");
    assert.equal(navy.hitPoints, CAREER_DOORS.navy.maxHp, "Navy starts at full career HP");
    assert.equal(navy.maxHp, CAREER_DOORS.navy.maxHp, "Navy deploys with 35 max HP");
    assert.equal(navy.psiPoints, CAREER_DOORS.navy.maxPsi, "Navy starts at full career psi");
    assert.equal(navy.maxPsi, CAREER_DOORS.navy.maxPsi, "Navy deploys with 35 max psi");

    const osa = await enlist("osa");
    assert.equal(osa.bit, "complete", "OSA door should set career_osa");
    assert.equal(osa.hitPoints, CAREER_DOORS.osa.maxHp, "OSA starts at full career HP");
    assert.equal(osa.maxHp, CAREER_DOORS.osa.maxHp, "OSA deploys with 30 max HP");
    assert.equal(osa.psiPoints, CAREER_DOORS.osa.maxPsi, "OSA starts at full career psi");
    assert.equal(osa.maxPsi, CAREER_DOORS.osa.maxPsi, "OSA deploys with 60 max psi");
  },
);
