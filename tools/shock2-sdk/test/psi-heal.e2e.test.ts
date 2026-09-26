import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
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

test(
  "Cerebro-stimulated Regeneration heals the caster and spends its tier",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

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

    await selectPsiPower(game, "PsiHeal");
    await pullTrigger(game);
    await game.step({ frames: 30 });

    player = (await game.info()).player;
    assert.equal(
      player.hit_points,
      hurtHp! + HP_PER_PSI * PSI_STAT,
      "the cast heals 2 HP per point of PSI",
    );
    assert.equal(player.psi_points, startPsi! - PSI_COST, "a tier 2 cast costs two psi points");

    // Keep casting until the player is topped up. The last cast is clamped
    // (fewer HP are missing than the heal restores) and still costs its tier.
    const psiBeforeTopUp = player.psi_points!;
    let topUpCasts = 0;
    while (player.hit_points !== maxHp && topUpCasts < 5) {
      await pullTrigger(game);
      await game.step({ frames: 30 });
      player = (await game.info()).player;
      topUpCasts += 1;
    }
    assert.ok(topUpCasts > 0, "the top-up should need at least one clamped cast");
    assert.equal(player.hit_points, maxHp, "the heal never overshoots the maximum");
    assert.equal(
      player.psi_points,
      psiBeforeTopUp - PSI_COST * topUpCasts,
      "a clamped cast still costs its tier",
    );
    const fullPsi = player.psi_points;

    await pullTrigger(game);
    await game.step({ frames: 30 });
    player = (await game.info()).player;
    assert.equal(player.hit_points, maxHp, "a cast at full health heals nothing");
    assert.equal(player.psi_points, fullPsi, "a cast at full health spends nothing");
  },
);
