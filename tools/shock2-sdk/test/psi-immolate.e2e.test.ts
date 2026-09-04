import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

// Localized Pyrokinesis ("Immolate" in the gamesys, tier 2 sustained power):
// while active the caster burns, damaging creatures inside the aura authored
// on the power (StimSource Incendiary, intensity 5, radius 4) while its own
// Amplify 0.0 receptron keeps the caster fireproof.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const IMMOLATE_COST = 2;

/** Cycle the psi power selection until Immolate is selected (bounded). */
async function selectImmolate(game: GameServer): Promise<void> {
  for (let i = 0; i < 40; i++) {
    if ((await game.info()).player.selected_psi_power === "Immolate") return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 1 });
  }
  assert.fail("could not cycle the psi power selection to Immolate");
}

async function hitPoints(game: GameServer, entityId: number): Promise<number> {
  const detail = await game.entities.detail(entityId);
  const hp = detail.properties.find((p: { name: string }) => p.name === "HitPoints");
  assert.ok(hp, `entity ${entityId} should expose HitPoints`);
  return Number(hp.value);
}

test(
  "Immolate burns creatures in its radius and leaves the caster unharmed",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    const { entities } = await game.entities.list({ filter: "OG-Pipe" });
    assert.equal(entities.length, 2, "debug_psi pens two pipe hybrids");
    const [near, far] = [...entities].sort((a, b) => a.distance - b.distance);
    const nearStart = await hitPoints(game, near.id);
    const farStart = await hitPoints(game, far.id);

    // Cast it: 2 psi and the power joins the active list.
    await selectImmolate(game);
    const startPlayer = (await game.info()).player;
    await pullTrigger(game);
    await game.step({ frames: 30 });
    let player = (await game.info()).player;
    assert.equal(
      player.psi_points,
      startPlayer.psi_points! - IMMOLATE_COST,
      "Immolate costs 2 psi points",
    );
    assert.deepEqual(player.active_psi_powers, ["Immolate"]);

    // Out here at the spawn point, ~9 units from the pen, nothing burns.
    await game.step({ frames: 120 });
    assert.equal(await hitPoints(game, near.id), nearStart, "no burn outside the 4-unit radius");

    // Step into the pen, beside the near hybrid (~2 units) and outside the
    // radius of the far one (~4.7).
    await game.player.teleport({ x: -8.2, y: 1.2, z: -1.8 });
    await game.step({ frames: 200 });

    const nearAfter = await hitPoints(game, near.id);
    assert.ok(
      nearAfter < nearStart,
      `the hybrid in the aura should burn (was ${nearStart}, now ${nearAfter})`,
    );
    assert.equal(
      await hitPoints(game, far.id),
      farStart,
      "the hybrid outside the radius is untouched",
    );

    player = (await game.info()).player;
    assert.equal(
      player.hit_points,
      startPlayer.hit_points,
      "the caster is immune to their own fire (Amplify 0.0)",
    );
    assert.deepEqual(player.active_psi_powers, ["Immolate"], "still burning");
  },
);
