import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// In slowed time only bullets leave smoke: the hybrid's and the turret's slugs
// trail, the droid's laser bolt and the midwife's shot do not.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Shooter -> [round, trails?]
const ROUNDS: Record<string, [string, boolean]> = {
  "OG-Shotgun": ["Grunt Shotgun Slug", true],
  Security: ["Droid Laser Bolt", false],
  Midwife: ["Midwife Shot", false],
  "Slug Turret": ["Turret Slug", true],
};

for (const [shooterName, [roundName, trails]] of Object.entries(ROUNDS)) {
  test(
    `in slow time, ${roundName} ${trails ? "leaves" : "leaves no"} smoke`,
    { skip: !e2eEnabled, timeout: 300_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_ai_ranged" });
      await game.devParams.set("cheat", 1);
      await game.devParams.set("world_time_scale", 0.25);
      await game.step({ frames: 2 });

      // One shooter alive, so no other round blocks its line of fire.
      const all = (await game.entities.list({ limit: 400 })).entities;
      const shooter = all.find((e) => e.name === shooterName);
      assert.ok(shooter, `debug_ai_ranged should contain ${shooterName}`);
      for (const other of all.filter(
        (e) => e.name in ROUNDS && e.id !== shooter.id,
      )) {
        await game.entities.sendMessage(other.id, { type: "Damage", amount: 10_000 });
      }

      // The filter is a wildcard: "Turret Slug" also finds the turret's
      // "Turret Slug Thrower", so match names exactly.
      const rounds = async () =>
        (await game.entities.list({ filter: roundName, limit: 10 })).entities;
      let round: number | undefined;
      let smoked = false;
      for (let frame = 0; frame < 1200; frame++) {
        await game.step({ frames: 1 });
        if (round === undefined) {
          round = (await rounds()).find((e) => e.name === roundName)?.id;
          continue;
        }
        smoked = (await game.scene.fromSource("projectile_smoke")).some(
          (o) => o.entity_id === round,
        );
        // Watch the round's whole flight.
        if (smoked || !(await rounds()).some((e) => e.id === round)) break;
      }

      assert.ok(round !== undefined, `${shooterName} must fire within the window`);
      assert.equal(smoked, trails, `${roundName} smoke`);
    },
  );
}
