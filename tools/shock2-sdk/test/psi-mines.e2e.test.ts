import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { hitPoints } from "./helpers/entity.js";
import { selectPsiPower } from "./helpers/psi.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for PsiMines (External Psionic Detonation, tier 5): the cast
// lobs a mine, the mine trips on a creature, and the authored explosion hurts
// it.
//
// Negative-first: before the `PsiMine` script was registered, the cast spawned
// `PsiMine Projectile`, whose authored script fell through to
// `PanicOnLoadScript` and killed the process - so every request after the cast
// here failed outright (#1295).
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "a psi mine detonates on the creature it is thrown at",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    await selectPsiPower(game, "PsiMines");

    // Something to trip the mine: the debug spawn drops an OG-Pipe hybrid a
    // few units ahead of the player.
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 30 });
    const [creature] = (await game.entities.list({ filter: "OG-Pipe", limit: 10 })).entities;
    assert.ok(creature, "SpawnDebugMonster should put a creature in front of the player");
    const healthBefore = hitPoints(await game.entities.detail(creature.id));

    const startPsi = (await game.info()).player.psi_points;
    assert.ok(startPsi !== null && startPsi >= 5, `need psi to cast (got ${startPsi})`);

    // `fireOnce` steps two frames, so the mine is still in its arming delay
    // here - a creature this close trips it as soon as it goes live.
    await fireOnce(game);

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

    // The mine reaches the creature and trips on it.
    let entities = (await game.entities.list({ limit: 200 })).entities;
    for (let waited = 0; waited < 8 && entities.some((e) => e.id === mineId); waited += 1) {
      await game.step({ frames: 15 });
      entities = (await game.entities.list({ limit: 200 })).entities;
    }
    assert.ok(
      !entities.some((entity) => entity.id === mineId),
      "the mine detonates on the creature instead of drifting past it",
    );
    assert.ok(
      entities.some((entity) => entity.name === "Psi Mine Explosion"),
      "slaying the mine spawns its authored Psi Mine Explosion",
    );

    // ...and the blast it spawns (30 @ r4 of Psi Stim) hurts what tripped it.
    const detail = await game.entities.detail(creature.id).catch(() => null);
    const healthAfter = detail === null ? 0 : hitPoints(detail);
    assert.ok(
      healthAfter < healthBefore,
      `the blast damages the creature (${healthBefore} -> ${healthAfter})`,
    );
  },
);
