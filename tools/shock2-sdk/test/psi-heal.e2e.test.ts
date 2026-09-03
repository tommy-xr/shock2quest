import assert from "node:assert/strict";
import { test } from "node:test";

import type { GameServer } from "../src/index.js";
import { GameServer as Server } from "../src/index.js";
import { pullTrigger } from "./helpers/weapon.js";

// End-to-end test for Cerebro-stimulated Regeneration (PsiHeal), the first
// instant (activation type 2) psi power: a cast spends the power's tier and
// heals `data[0] + data[1] x PSI` HP, clamped to the player's missing health.
// A cast at full health is refused and spends nothing.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** PsiHeal's authored data is `[0, 2]`: 2 HP per point of PSI. */
const HP_PER_PSI = 2;
const PSI_STAT = 6;
const PSI_COST = 2;

/** Cycle the psi selection until `name` is selected. */
async function selectPower(game: GameServer, name: string): Promise<void> {
  for (let i = 0; i < 40; i += 1) {
    if ((await game.info()).player.selected_psi_power === name) return;
    await game.input.trigger("CyclePsiPower");
    await game.step({ frames: 2 });
  }
  throw new Error(`CyclePsiPower never reached ${name}`);
}

test(
  "Cerebro-stimulated Regeneration heals the caster and spends its tier",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await Server.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: PSI_STAT });

    // Hurt the player so the heal has room to land.
    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null, "debug_psi should have a player");
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 20.0 });
    await game.step({ frames: 10 });

    let player = (await game.info()).player;
    const maxHp = player.max_hit_points;
    const hurtHp = player.hit_points;
    const startPsi = player.psi_points;
    assert.ok(maxHp !== null && hurtHp !== null && startPsi !== null);
    assert.ok(hurtHp! < maxHp!, `player should be hurt (${hurtHp}/${maxHp})`);
    assert.ok(maxHp! - hurtHp! > HP_PER_PSI * PSI_STAT, "the heal should not be clamped here");

    await selectPower(game, "PsiHeal");
    await pullTrigger(game);
    await game.step({ frames: 30 });

    player = (await game.info()).player;
    assert.equal(
      player.hit_points,
      hurtHp! + HP_PER_PSI * PSI_STAT,
      "the cast heals 2 HP per point of PSI",
    );
    assert.equal(player.psi_points, startPsi! - PSI_COST, "a tier 2 cast costs two psi points");

    // Keep casting until the player is topped up (the last cast clamps), then
    // cast at full health: refused, and nothing is spent.
    for (let i = 0; i < 5 && player.hit_points !== maxHp; i += 1) {
      await pullTrigger(game);
      await game.step({ frames: 30 });
      player = (await game.info()).player;
    }
    assert.equal(player.hit_points, maxHp, "repeated casts top the player up to the maximum");
    const fullPsi = player.psi_points;

    await pullTrigger(game);
    await game.step({ frames: 30 });
    player = (await game.info()).player;
    assert.equal(player.hit_points, maxHp, "a cast at full health heals nothing");
    assert.equal(player.psi_points, fullPsi, "a cast at full health spends nothing");
  },
);

test(
  "the heal is clamped to the player's missing health",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await Server.launch({ mission: "debug_psi" });

    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: PSI_STAT });

    // Only a few HP missing: the 12-HP heal tops out at the maximum.
    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null);
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 3.0 });
    await game.step({ frames: 10 });

    let player = (await game.info()).player;
    const maxHp = player.max_hit_points;
    const startPsi = player.psi_points;
    assert.ok(maxHp !== null && player.hit_points !== null && startPsi !== null);
    assert.ok(
      maxHp! - player.hit_points! < HP_PER_PSI * PSI_STAT,
      "fewer HP should be missing than the heal would restore",
    );

    await selectPower(game, "PsiHeal");
    await pullTrigger(game);
    await game.step({ frames: 30 });

    player = (await game.info()).player;
    assert.equal(player.hit_points, maxHp, "the heal never overshoots the maximum");
    assert.equal(player.psi_points, startPsi! - PSI_COST, "a clamped cast still costs its tier");
  },
);
