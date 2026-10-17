import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// A creature's shot shows a muzzle flash and barrel smoke at its weapon, not
// just the bare projectile. `debug_ai_ranged` stands a shotgun hybrid, a
// midwife and a security droid ahead of the player. Each case keeps one
// shooter alive: another's round in flight blocks a line of fire, which can
// hold a shooter's trigger for the whole window.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const ROUNDS: Record<string, string> = {
  "OG-Shotgun": "Grunt Shotgun Slug",
  Midwife: "Midwife Shot",
  Security: "Droid Laser Bolt",
};

for (const [shooterName, roundName] of Object.entries(ROUNDS)) {
  test(
    `${shooterName} fires with a muzzle flash and barrel smoke`,
    { skip: !e2eEnabled, timeout: 300_000 },
    async () => {
      await using game = await GameServer.launch({ mission: "debug_ai_ranged" });
      await game.devParams.set("cheat", 1);
      await game.step({ frames: 2 });

      const all = (await game.entities.list({ limit: 400 })).entities;
      const shooter = all.find((e) => e.name === shooterName);
      assert.ok(shooter, `debug_ai_ranged should contain ${shooterName}`);
      for (const other of all.filter((e) => e.name in ROUNDS && e.id !== shooter.id)) {
        await game.entities.sendMessage(other.id, { type: "Damage", amount: 10_000 });
      }
      const rounds = new Set(all.filter((e) => e.name === roundName).map((e) => e.id));

      let firedAt: number | undefined;
      let smoke = 0;
      let slugFlashToShooter: number | undefined;
      for (let frame = 0; frame < 600; frame++) {
        await game.step({ frames: 1 });
        const entities = (await game.entities.list({ limit: 400 })).entities;
        if (
          firedAt === undefined &&
          entities.some((e) => e.name === roundName && !rounds.has(e.id))
        ) {
          firedAt = frame;
        }
        const flash = entities.find((e) => e.name === "Slug Flash");
        if (flash && slugFlashToShooter === undefined) {
          const at = (await game.entities.detail(shooter.id)).position;
          slugFlashToShooter = Math.hypot(
            flash.position[0] - at[0],
            flash.position[1] - at[1],
            flash.position[2] - at[2],
          );
        }
        smoke = Math.max(
          smoke,
          (await game.scene.fromSource("barrel_smoke")).filter(
            (o) => o.entity_id === shooter.id,
          ).length,
        );
        if (firedAt !== undefined && frame > firedAt + 2) break;
      }

      assert.ok(firedAt !== undefined, `${shooterName} must fire within the window`);
      assert.ok(smoke > 0, `${shooterName}'s shot must leave barrel smoke tagged to it`);
      if (shooterName === "OG-Shotgun") {
        // At the hybrid's gun, short of where its round spawns.
        assert.ok(
          slugFlashToShooter !== undefined && slugFlashToShooter < 3.0,
          `the hybrid's shot must spawn the authored Slug Flash at its gun (${slugFlashToShooter})`,
        );
      } else {
        assert.equal(slugFlashToShooter, undefined, "only the slug round has an authored flash");
      }
    },
  );
}
