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

function distance(a: [number, number, number], b: [number, number, number]): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

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
      let roundToShooter: number | undefined;
      let slugFlashToShooter: number | undefined;
      const sprites = new Set<string>();
      for (let frame = 0; frame < 600; frame++) {
        await game.step({ frames: 1 });
        const entities = (await game.entities.list({ limit: 400 })).entities;
        const at = (await game.entities.detail(shooter.id)).position;
        const round = entities.find((e) => e.name === roundName && !rounds.has(e.id));
        if (firedAt === undefined && round) {
          firedAt = frame;
          roundToShooter = distance(round.position, at);
        }
        const flash = entities.find((e) => e.name === "Slug Flash");
        if (flash && slugFlashToShooter === undefined) {
          slugFlashToShooter = distance(flash.position, at);
        }
        for (const object of await game.scene.fromSource("barrel_smoke")) {
          if (object.entity_id === shooter.id && object.model) sprites.add(object.model);
        }
        if (firedAt !== undefined && frame > firedAt + 2) break;
      }

      assert.ok(firedAt !== undefined, `${shooterName} must fire within the window`);
      assert.ok(sprites.has("NDsmk"), `${shooterName}'s shot must leave barrel smoke`);
      if (shooterName === "OG-Shotgun") {
        // Ahead of the body at the gun, and short of where the round spawns
        // (a clearance unit beyond the launch joint).
        assert.ok(
          slugFlashToShooter !== undefined &&
            slugFlashToShooter > 1.5 &&
            slugFlashToShooter < roundToShooter!,
          `the authored Slug Flash must sit at the hybrid's gun (flash ${slugFlashToShooter}, round ${roundToShooter})`,
        );
        assert.ok(!sprites.has("NDsbll"), "an authored flash replaces the sprite flash");
      } else {
        assert.equal(slugFlashToShooter, undefined, "only the slug round has an authored flash");
        assert.ok(sprites.has("NDsbll"), `${shooterName}'s shot must show a sprite flash`);
      }
    },
  );
}
