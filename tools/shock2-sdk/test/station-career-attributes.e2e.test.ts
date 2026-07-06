import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for station career (Marine/Navy/OSA) branching. Requires game
// assets in Data/ and compiles the runtime on first run, so it is opt-in:
//
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
//
// Negative-first: before this change, choosing a service branch did nothing -
// there were no SelectCareer* actions and no career loadout, so all three tours
// deployed to medsci1 with the identical default player-template attributes
// (30 HP, 40/50 psi). The asserts that the three careers arrive with *different*
// hit points and psi points would all fail (every value would be 30/40).
//
// After the change, the career is registered (persisted quest bit) and applied
// on deployment, so Marines arrive tanky (45 HP / 20 psi), Navy balanced
// (35 / 35), and OSA psionic (30 / 60).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Select a career at the station, deploy to medsci1, and read the arrival
 * attributes. transitionLevel() reloads medsci1, so the loadout (which applies
 * on mission load from the persisted career bit) re-applies each time. */
async function deployWithCareer(
  game: GameServer,
  action: string,
): Promise<{ maxHp: number; maxPsi: number }> {
  await game.input.trigger(action);
  await game.step({ frames: 2 });
  await game.transitionLevel("medsci1.mis");
  await game.step({ frames: 3 });
  const player = (await game.info()).player;
  assert.notEqual(
    player.max_hit_points,
    null,
    `${action}: player should have a hit-point pool after deployment`,
  );
  assert.notEqual(
    player.max_psi_points,
    null,
    `${action}: player should have a psi pool after deployment`,
  );
  return {
    maxHp: player.max_hit_points as number,
    maxPsi: player.max_psi_points as number,
  };
}

test(
  "station: careers deploy to medsci1 with distinct attributes",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8117),
    });
    await game.step({ frames: 10 });

    const marine = await deployWithCareer(game, "SelectCareerMarine");
    const navy = await deployWithCareer(game, "SelectCareerNavy");
    const osa = await deployWithCareer(game, "SelectCareerOsa");

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
