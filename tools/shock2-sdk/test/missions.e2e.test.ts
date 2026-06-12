import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Smoke test: every mission in Data/ should load, step, and contain entities.
// Opt-in (compiles the runtime on first use, loads every level):
//
//   npm run test:e2e
//   # or just this file:
//   tsc && SHOCK2_E2E=1 node --test dist/test/missions.e2e.test.js
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const MISSIONS = [
  "earth.mis",
  "station.mis",
  "medsci1.mis",
  "medsci2.mis",
  "eng1.mis",
  "eng2.mis",
  "hydro1.mis",
  "hydro2.mis",
  "hydro3.mis",
  "ops1.mis",
  "ops2.mis",
  "ops3.mis",
  "ops4.mis",
  "rec1.mis",
  "rec2.mis",
  "rec3.mis",
  "command1.mis",
  "command2.mis",
  "rick1.mis",
  "rick2.mis",
  "rick3.mis",
  "many.mis",
  // Known issue: AIPATH path database parser panics with unexpected EOF on
  // this mission. Re-enable once fixed.
  // https://github.com/tommy-xr/shock2quest/issues/267
  // "shodan.mis",
];

const BASE_PORT = Number(process.env.SHOCK2_E2E_PORT ?? 8100);

MISSIONS.forEach((mission, index) => {
  test(
    `mission loads: ${mission}`,
    { skip: !e2eEnabled, timeout: 300_000 },
    async () => {
      await using game = await GameServer.launch({
        mission,
        port: BASE_PORT + index,
      });

      // Simulation advances without crashing.
      const step = await game.step({ frames: 2 });
      assert.equal(step.frames_advanced, 2);

      // The right mission is running and entities were instantiated.
      const info = await game.info();
      assert.equal(info.mission, mission);
      assert.ok(
        info.entity_count > 0,
        `expected entities in ${mission}, got ${info.entity_count}`,
      );

      // Player spawned at a finite position.
      const pos = await game.player.position();
      for (const value of [pos.x, pos.y, pos.z]) {
        assert.ok(Number.isFinite(value), `non-finite player position in ${mission}`);
      }
    },
  );
});
