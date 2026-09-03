import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult } from "../src/index.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for PsiMines (External Psionic Detonation, tier 5).
//
// Negative-first: before the `PsiMine` script was registered, the cast spawned
// `PsiMine Projectile`, whose authored script fell through to
// `PanicOnLoadScript` and killed the process - so every request after the cast
// here failed outright (#1295).
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Cycle the psi power selection until `name` is selected. */
async function selectPower(game: GameServer, name: string): Promise<void> {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  throw new Error(`psi power ${name} was never selected`);
}

function hitPoints(detail: EntityDetailResult): number {
  const property = detail.properties.find((p) => p.name === "HitPoints");
  assert.ok(property, `entity ${detail.entity_id} should expose HitPoints`);
  return Number(property.value);
}

test(
  "casting PsiMines survives, spends its tier, and the mine detonates",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    await selectPower(game, "PsiMines");

    const startPsi = (await game.info()).player.psi_points;
    assert.ok(startPsi !== null && startPsi >= 5, `need psi to cast (got ${startPsi})`);

    // Baseline health of whatever creatures the scene pens up ahead of the
    // player, before anything explodes.
    const creatures = (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities;
    const healthBefore = new Map<number, number>();
    for (const creature of creatures) {
      healthBefore.set(creature.id, hitPoints(await game.entities.detail(creature.id)));
    }
    // Lob the mine over the pen wall at the nearest creature's head, so the
    // mine's flight brings it inside its own trigger radius.
    if (creatures[0]) {
      const [x, y, z] = creatures[0].position;
      await game.input.lookAtWorldPoint([x, y + 1.7, z]);
    }

    await fireOnce(game);
    await game.step({ frames: 4 });

    // The runtime is still alive to answer at all - the regression test for the
    // panic - and the cast spent the power's authored tier.
    assert.equal(
      (await game.info()).player.psi_points,
      startPsi! - 5,
      "a tier 5 cast costs five psi points",
    );

    const mines = (await game.entities.list({ limit: 200 })).entities.filter(
      (entity) => entity.name === "PsiMine Projectile",
    );
    assert.equal(mines.length, 1, "the cast lobs one PsiMine Projectile");
    const mineId = mines[0].id;

    const stillFlying = async () =>
      (await game.entities.list({ limit: 200 })).entities.some((e) => e.id === mineId);

    // Give the mine time to reach the creatures and trip on one of them...
    for (let waited = 0; waited < 8 && (await stillFlying()); waited += 1) {
      await game.step({ frames: 15 });
    }
    const trippedOnACreature = !(await stillFlying());
    if (!trippedOnACreature) {
      // ...and if the scene has nothing to trip it, a damaged mine goes off
      // where it lies.
      await game.entities.sendMessage(mineId, { type: "Damage", amount: 1 });
      await game.step({ frames: 5 });
    }

    const entities = (await game.entities.list({ limit: 200 })).entities;
    assert.ok(
      !entities.some((entity) => entity.id === mineId),
      "the detonated mine is gone",
    );
    assert.ok(
      entities.some((entity) => entity.name === "Psi Mine Explosion"),
      "slaying the mine spawns its authored Psi Mine Explosion",
    );

    // Only a creature can have tripped the mine, and the blast it spawns
    // (30 @ r4 of Psi Stim) reaches whatever was that close.
    if (trippedOnACreature) {
      let damaged = 0;
      for (const [id, before] of healthBefore) {
        const detail = await game.entities.detail(id).catch(() => null);
        if (detail && hitPoints(detail) < before) damaged += 1;
      }
      assert.ok(damaged > 0, "the blast damages the creature that tripped the mine");
    }
  },
);
