import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";

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
// Two assertions deliberately pin CURRENT (buggy) behavior so they FLIP when the
// underlying issue is fixed:
//   * #453 - training tours grant NO stat changes. We assert hp/psi are
//     UNCHANGED across the tours; flip to "increases" when #453 lands.
//   * #454 - post-career station arrival spawns at world origin (0,-0.87,0)
//     instead of the designed recruit-deck spot (81.78,-3.6,16.54). We assert
//     the buggy origin; flip to the designed spot when #454 lands.
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

const CAREER_SPAWN_BUG: Vec3 = [0.0, -0.87, 0.0]; // #454
const near = (a: number, b: number) => Math.abs(a - b) < 0.05;

async function teleportTo(game: GameServer, [x, y, z]: Vec3): Promise<void> {
  await game.player.teleport({ x, y, z });
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
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8130),
    });
    await game.step({ frames: 5 });
    await game.screenshot("station-flow-earth.png");

    // Step 1: enlist as a Marine by walking through the Marine career door.
    // Teleport ONTO its tripwire volume (equivalent to walking in: fires the same
    // ENTER trigger) and let the real TrapNewTripwire -> ChooseService chain run.
    await teleportTo(game, CAREER_DOORS.marine.trip);
    await game.step({ frames: 15 });

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
    assert.equal(info.player.max_hit_points, 45, "Marine deploys with 45 max HP");
    assert.equal(info.player.max_psi_points, 20, "Marine deploys with 20 max psi");
    // #454: post-career arrival currently spawns at world origin, not the
    // designed recruit-deck spot (81.78,-3.6,16.54). Flip this to the designed
    // spot when #454 is fixed.
    const arrival = info.player.position;
    assert.ok(
      near(arrival[0], CAREER_SPAWN_BUG[0]) &&
        near(arrival[1], CAREER_SPAWN_BUG[1]) &&
        near(arrival[2], CAREER_SPAWN_BUG[2]),
      `#454: post-career arrival should be at the buggy origin ${JSON.stringify(
        CAREER_SPAWN_BUG,
      )}, got ${JSON.stringify(arrival)}`,
    );

    // Step 2: tours 1 and 2 each set exactly the next training_year bit and loop
    // back to station.mis. hp/psi must NOT change (#453).
    const tours: { expected: string[] }[] = [
      { expected: ["training_year_2"] },
      { expected: ["training_year_2", "training_year_3"] },
    ];
    for (const [i, tour] of tours.entries()) {
      await teleportTo(game, TOUR_TRIP);
      await game.step({ frames: 20 });
      info = await game.info();
      assert.equal(
        info.mission.toLowerCase(),
        "station.mis",
        `tour ${i + 1} (year < 4) should loop back to station.mis`,
      );
      assert.deepEqual(
        await completeTrainingYears(game),
        tour.expected,
        `tour ${i + 1} should set exactly ${JSON.stringify(tour.expected)}`,
      );
      // #453: tours grant no stats today. Flip to an increase when #453 is fixed.
      assert.equal(info.player.max_hit_points, 45, `#453: tour ${i + 1} must not change max HP`);
      assert.equal(info.player.max_psi_points, 20, `#453: tour ${i + 1} must not change max psi`);
    }

    // Step 3: the third tour advances to year 4 and deploys to MedSci1.
    await teleportTo(game, TOUR_TRIP);
    await game.step({ frames: 20 });
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
    // Career and loadout survive the whole chain unchanged (#453 keeps stats flat).
    assert.equal(await game.quests.get("career_marine"), "complete", "Marine career should survive deploy");
    assert.equal(info.player.max_hit_points, 45, "Marine max HP should survive deploy");
    assert.equal(info.player.max_psi_points, 20, "Marine max psi should survive deploy");
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
  port: number,
): Promise<{ bit: string; maxHp: number | null; maxPsi: number | null }> {
  const door = CAREER_DOORS[branch];
  await using game = await GameServer.launch({ mission: "earth.mis", port });
  await game.step({ frames: 5 });

  await teleportTo(game, door.trip);
  await game.step({ frames: 15 });

  const info = await game.info();
  assert.equal(
    info.mission.toLowerCase(),
    "station.mis",
    `${branch}: entering the career door should transition to the recruit station`,
  );
  return {
    bit: await game.quests.get(door.bit),
    maxHp: info.player.max_hit_points,
    maxPsi: info.player.max_psi_points,
  };
}

test(
  "station flow: Navy and OSA doors map to the correct career bit and loadout",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8131);

    // Navy: the door whose tripwire switches the mislabeled "SendToMarines"
    // marker (P$Service=1). Correct engine behavior yields the Navy career.
    const navy = await enlist("navy", basePort);
    assert.equal(navy.bit, "complete", "Navy door should set career_navy (despite the 'SendToMarines' misnomer)");
    assert.equal(navy.maxHp, CAREER_DOORS.navy.maxHp, "Navy deploys with 35 max HP");
    assert.equal(navy.maxPsi, CAREER_DOORS.navy.maxPsi, "Navy deploys with 35 max psi");

    const osa = await enlist("osa", basePort + 1);
    assert.equal(osa.bit, "complete", "OSA door should set career_osa");
    assert.equal(osa.maxHp, CAREER_DOORS.osa.maxHp, "OSA deploys with 30 max HP");
    assert.equal(osa.maxPsi, CAREER_DOORS.osa.maxPsi, "OSA deploys with 60 max psi");
  },
);
