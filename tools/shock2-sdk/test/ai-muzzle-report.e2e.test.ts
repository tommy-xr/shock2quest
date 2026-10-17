import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// A creature's shot shows its report at the weapon, not just the bare
// projectile: the hybrid's flash and smoke, the droid's quieter laser flash
// and wisp, and the midwife's faint wisp. `debug_ai_ranged` stands a shotgun
// hybrid, a midwife and a security droid ahead of the player. Each case keeps one
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
    `${shooterName} fires with its muzzle report`,
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
      let flashToShooter: number | undefined;
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
        const flash = entities.find((e) => e.name === "Assault Flash");
        if (flash && flashToShooter === undefined) {
          flashToShooter = distance(flash.position, at);
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
          flashToShooter !== undefined &&
            flashToShooter > 1.5 &&
            flashToShooter < roundToShooter!,
          `the hybrid's Assault Flash must sit at its gun (flash ${flashToShooter}, round ${roundToShooter})`,
        );
        assert.ok(!sprites.has("NDsbll"), "an authored flash replaces the sprite flash");
      } else {
        assert.equal(flashToShooter, undefined, "only the hybrid's shot spawns a flash entity");
        // The droid's laser is quieter than the hybrid; the midwife's organic
        // weapon leaves only a faint wisp, with no flash at all.
        assert.equal(
          sprites.has("NDsbll"),
          shooterName === "Security",
          `${shooterName}'s sprite flash`,
        );
      }
    },
  );
}
