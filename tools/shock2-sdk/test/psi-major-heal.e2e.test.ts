import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

// End-to-end test for Advanced Cerebro-stimulated Regeneration (Major Heal),
// the tier-5 instant self-heal: a cast spends 5 psi points and heals
// `data[0] + data[1] x PSI` HP, clamped to the player's missing health. A
// cast at full health is refused and spends nothing.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** Major Heal's authored data is `[5, 5]`: 5 HP + 5 per point of PSI. */
const HP_BASE = 5;
const HP_PER_PSI = 5;
const PSI_STAT = 6;
const PSI_COST = 5;

test(
  "Advanced Cerebro-stimulated Regeneration heals the caster and spends its tier",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    await game.player.setStats({ psionic_ability: PSI_STAT });

    // Hurt the player so the heal has room to land.
    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null, "debug_psi should have a player");
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 25.0 });
    await game.step({ frames: 10 });

    let player = (await game.info()).player;
    const maxHp = player.max_hit_points;
    const hurtHp = player.hit_points;
    const startPsi = player.psi_points;
    assert.ok(maxHp !== null && hurtHp !== null && startPsi !== null);
    assert.ok(hurtHp! < maxHp!, `player should be hurt (${hurtHp}/${maxHp})`);

    await selectPsiPower(game, "Major Heal");
    await pullTrigger(game);
    await game.step({ frames: 30 });

    const expectedHeal = Math.min(HP_BASE + HP_PER_PSI * PSI_STAT, maxHp! - hurtHp!);
    player = (await game.info()).player;
    assert.equal(
      player.hit_points,
      hurtHp! + expectedHeal,
      "the cast heals 5 + 5 per point of PSI, clamped to the missing health",
    );
    assert.equal(player.psi_points, startPsi! - PSI_COST, "a tier 5 cast costs five psi points");

    // A cast with nothing left to heal is refused outright.
    assert.equal(player.hit_points, maxHp, "the heal tops the player up here");
    const fullPsi = player.psi_points;
    await pullTrigger(game);
    await game.step({ frames: 30 });
    player = (await game.info()).player;
    assert.equal(player.hit_points, maxHp, "a cast at full health heals nothing");
    assert.equal(player.psi_points, fullPsi, "a cast at full health spends nothing");
  },
);
