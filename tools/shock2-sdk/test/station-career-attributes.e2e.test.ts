import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { Vec3 } from "../src/types.js";
import { stepPastCutscenes } from "./helpers/cutscenes.js";

// End-to-end test for career (Marine/Navy/OSA) branching. Requires game assets
// in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// The career is chosen by *playing the recruitment intro* (earth.mis): three
// ChooseService markers there carry a P$Service value (0=Marine, 1=Navy, 2=OSA)
// and a transition to the recruit station. TurnOn'ing one (as its tripwire does
// when the player walks through that career door) persists the chosen career as
// a quest bit and ships the player to the station; the career loadout is then
// applied to the player on every subsequent mission load. Marines arrive tanky
// (45 HP / 20 psi), Navy balanced (35 / 35), OSA psionic (30 / 60).
//
// Negative-first: with the loadout neutralized (or the career never registered),
// all three branches deploy with the identical default player-template
// attributes (30 HP, 40/50 psi) and the "should differ" asserts fail. The test
// also guards that selection is the *station flow*, not the removed debug
// SelectCareer* input actions.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// The ChooseService career markers in earth.mis, keyed by their P$Service value,
// located by their (stable) mission-data world positions. Runtime entity ids are
// not stable across runs and two of the three markers are unnamed, so position
// is the reliable handle.
const MARKERS: Record<string, { service: number; pos: Vec3 }> = {
  marine: { service: 0, pos: [-5.199172, 24.0, 68.71187] },
  navy: { service: 1, pos: [-1.8059494, 24.0, 84.12585] },
  osa: { service: 2, pos: [16.933895, 24.0, 83.833176] },
};

function dist2(a: Vec3, b: Vec3): number {
  return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
}

/**
 * Play one career: launch the recruitment intro, TurnOn the branch's
 * ChooseService marker (the real career-choice entity), confirm it ships the
 * player to the recruit station, deploy on to medsci1, and read the arrival
 * attributes. A fresh launch per career keeps the runs independent (and avoids a
 * pre-existing earth.mis reload issue).
 */
async function playCareer(
  branch: keyof typeof MARKERS,
): Promise<{ maxHp: number; maxPsi: number }> {
  await using game = await GameServer.launch({ mission: "earth.mis" });
  await game.step({ frames: 5 });

  // Guard: selection must go through station's own entities, not the removed
  // debug SelectCareer* actions.
  const actions = await game.input.actions();
  assert.ok(
    !actions.some((a) => a.startsWith("SelectCareer")),
    "career selection must not be driven by debug SelectCareer* actions",
  );

  const { pos } = MARKERS[branch];
  const { entities } = await game.entities.list({ limit: 5000 });
  const candidates = entities.filter((e) => e.position && e.position.every((v) => v != null));
  const marker = candidates.reduce((best, e) =>
    dist2(e.position, pos) < dist2(best.position, pos) ? e : best,
  );
  assert.ok(
    dist2(marker.position, pos) < 0.01,
    `${branch}: found a marker at ${JSON.stringify(marker.position)} (wanted ${JSON.stringify(pos)})`,
  );
  // Confirm the position lookup actually hit a ChooseService marker (the real
  // career-choice entity), not just some unrelated entity nearby.
  const detail = await game.entities.detail(marker.id);
  assert.ok(
    detail.properties.some((p) => p.name === "Scripts" && p.value.includes("ChooseService")),
    `${branch}: marker ${marker.id} should carry the ChooseService script`,
  );

  await game.entities.sendMessage(marker.id, { type: "TurnOn" });
  await game.step({ frames: 3 });
  // Enlisting plays its authored movie first (campaign-cutscenes.e2e.test.ts);
  // this test is about the loadout waiting on the other side of it.
  await stepPastCutscenes(game);
  // The marker's own transition ships the player to the recruit station.
  assert.equal(
    (await game.info()).mission,
    "station.mis",
    `${branch}: choosing a career should transition to the recruit station`,
  );

  // Deploy on to medsci1 (reloading re-applies the loadout from the persisted
  // career bit on mission load).
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 3 });
  const player = (await game.info()).player;
  assert.notEqual(player.max_hit_points, null, `${branch}: player should have a hit-point pool`);
  assert.notEqual(player.max_psi_points, null, `${branch}: player should have a psi pool`);
  return {
    maxHp: player.max_hit_points as number,
    maxPsi: player.max_psi_points as number,
  };
}

test(
  "career: branches chosen at the recruitment intro deploy with distinct attributes",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const marine = await playCareer("marine");
    const navy = await playCareer("navy");
    const osa = await playCareer("osa");

    // Hit points differ across all three branches (Marines tankiest, OSA least).
    assert.notEqual(marine.maxHp, navy.maxHp, "Marine vs Navy HP should differ");
    assert.notEqual(navy.maxHp, osa.maxHp, "Navy vs OSA HP should differ");
    assert.notEqual(marine.maxHp, osa.maxHp, "Marine vs OSA HP should differ");
    assert.ok(
      marine.maxHp > osa.maxHp,
      `Marines should be tankier than OSA (got ${marine.maxHp} vs ${osa.maxHp})`,
    );

    // Psi points differ too (OSA the strongest psion, Marines the weakest).
    assert.notEqual(marine.maxPsi, navy.maxPsi, "Marine vs Navy psi should differ");
    assert.notEqual(navy.maxPsi, osa.maxPsi, "Navy vs OSA psi should differ");
    assert.ok(
      osa.maxPsi > marine.maxPsi,
      `OSA should out-psi Marines (got ${osa.maxPsi} vs ${marine.maxPsi})`,
    );
  },
);
