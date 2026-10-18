import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// A creature's shot shows its report at the weapon, not just the bare
// projectile: the hybrid's flash and smoke, the droid's quieter laser flash
// and wisp, and the midwife's faint wisp, each with its gunshot.
// `debug_ai_ranged` stands a shotgun hybrid, a midwife, a security droid and a
// slug turret ahead of the player. Each case keeps one shooter alive: another's round in
// flight blocks a line of fire, which can hold a shooter's trigger for the
// whole window.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

// Shooter -> [round, gunshot sample resolved from the round's launch schema].
const ROUNDS: Record<string, [string, string]> = {
  "OG-Shotgun": ["Grunt Shotgun Slug", "ogsgun"],
  Midwife: ["Midwife Shot", "f_mwlas"],
  Security: ["Droid Laser Bolt", "fdrolas"],
};

function distance(a: [number, number, number], b: [number, number, number]): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

for (const [shooterName, [roundName, gunshot]] of Object.entries(ROUNDS)) {
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
      const others = all.filter(
        (e) => (e.name in ROUNDS || e.name === "Slug Turret") && e.id !== shooter.id,
      );
      for (const other of others) {
        await game.entities.sendMessage(other.id, { type: "Damage", amount: 10_000 });
      }
      const rounds = new Set(all.filter((e) => e.name === roundName).map((e) => e.id));

      let firedAt: number | undefined;
      let roundToShooter: number | undefined;
      let flashToShooter: number | undefined;
      let flashCulling: string | null | undefined;
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
          const { objects } = await game.scene.objects({ entityId: flash.id });
          flashCulling = objects[0]?.backface_culling;
        }
        for (const object of await game.scene.fromSource("barrel_smoke")) {
          if (object.entity_id === shooter.id && object.model) sprites.add(object.model);
        }
        if (firedAt !== undefined && frame > firedAt + 2) break;
      }

      assert.ok(firedAt !== undefined, `${shooterName} must fire within the window`);
      assert.ok(sprites.has("NDsmk"), `${shooterName}'s shot must leave barrel smoke`);
      const launches = (await game.audio.recent()).sounds.filter((s) =>
        s.tags.some(([tag, value]) => tag === "event" && value === "launch"),
      );
      assert.ok(
        launches.some((s) => s.sample.toLowerCase().startsWith(gunshot)),
        `${shooterName}'s shot must play its gunshot (${launches.map((s) => s.sample)})`,
      );
      if (shooterName === "OG-Shotgun") {
        // Ahead of the body at the gun, and short of where the round spawns
        // (a clearance unit beyond the launch joint).
        assert.ok(
          flashToShooter !== undefined &&
            flashToShooter > 1.5 &&
            flashToShooter < roundToShooter!,
          `the hybrid's Assault Flash must sit at its gun (flash ${flashToShooter}, round ${roundToShooter})`,
        );
        // The flash mesh faces its shooter; culled, the player would never see it.
        assert.equal(flashCulling, null, "the hybrid's flash must draw double-sided");
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

// The slug turret's own flash is authored FullBright: drawn unlit at full
// intensity, not a dull lit ribbon.
test(
  "Slug Turret's flash draws full bright",
  { skip: !e2eEnabled, timeout: 300_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ai_ranged" });
    await game.devParams.set("cheat", 1);
    await game.step({ frames: 2 });
    for (const other of (await game.entities.list({ limit: 400 })).entities) {
      if (other.name in ROUNDS) {
        await game.entities.sendMessage(other.id, {
          type: "Damage",
          amount: 10_000,
        });
      }
    }

    let fullBright: boolean[] | undefined;
    for (let frame = 0; frame < 600 && fullBright === undefined; frame++) {
      await game.step({ frames: 1 });
      const [flash] = (await game.entities.list({ filter: "Slug Flash" }))
        .entities;
      if (flash) {
        const { objects } = await game.scene.objects({ entityId: flash.id });
        fullBright = objects.map((object) => object.full_bright);
      }
    }

    assert.ok(fullBright?.length, "the slug turret must fire its Slug Flash");
    assert.ok(
      fullBright.every(Boolean),
      `Slug Flash must draw full bright (${fullBright})`,
    );
  },
);
