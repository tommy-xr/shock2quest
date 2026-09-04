import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

// End-to-end test for Soma Transference (SomaDrain), the tier-5 instant aimed
// drain: the creature under the amp's aim loses health and the caster gains
// it. With nothing living in range the cast is refused and spends nothing.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

/** SomaDrain's authored data `[10, 5, 5]`: damage / transfer / range. */
const DAMAGE = 10;
const TRANSFER = 5;
const PSI_COST = 5;

async function hitPoints(game: GameServer, entityId: number): Promise<number> {
  const detail = await game.entities.detail(entityId);
  const property = detail.properties.find((p) => p.name === "HitPoints");
  assert.ok(property, `entity ${entityId} should expose HitPoints`);
  return Number(property.value);
}

/** Spawn a hybrid ~4 world units ahead of the player - inside the drain's
 * authored 5-unit range - and return it. */
async function spawnTarget(game: GameServer) {
  const known = new Set(
    (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map((e) => e.id),
  );
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 10 });
  const monster = (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.find(
    (e) => !known.has(e.id),
  );
  assert.ok(monster, "SpawnDebugMonster should create a target");
  return monster;
}

test(
  "Soma Transference drains the creature under the aim and heals the caster",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    // The scene auto-equips the Psi Amp on the first update.
    await game.step({ frames: 10 });
    const monster = await spawnTarget(game);
    const monsterStartHp = await hitPoints(game, monster.id);
    assert.ok(monsterStartHp > DAMAGE, "the drain should not kill the target outright");

    // Hurt the player so the transferred health is observable.
    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null, "debug_psi should have a player");
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 20.0 });
    await game.step({ frames: 10 });

    let player = (await game.info()).player;
    const hurtHp = player.hit_points;
    const startPsi = player.psi_points;
    assert.ok(hurtHp !== null && startPsi !== null);
    assert.ok(
      player.max_hit_points !== null && player.max_hit_points! - hurtHp! > TRANSFER,
      "the transfer should not be clamped by the player's missing health",
    );

    await selectPsiPower(game, "SomaDrain");
    await pullTrigger(game);
    await game.step({ frames: 30 });

    player = (await game.info()).player;
    assert.equal(player.psi_points, startPsi! - PSI_COST, "a tier 5 cast costs five psi points");
    assert.equal(
      await hitPoints(game, monster.id),
      monsterStartHp - DAMAGE,
      "the drained creature loses the authored damage",
    );
    assert.equal(
      player.hit_points,
      hurtHp! + TRANSFER,
      "the caster gains the transferred health",
    );
  },
);

test(
  "a creature beyond the authored range is out of reach",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    await game.step({ frames: 10 });
    const monster = await spawnTarget(game);
    const monsterStartHp = await hitPoints(game, monster.id);

    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null);
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 20.0 });
    await game.step({ frames: 10 });

    await selectPsiPower(game, "SomaDrain");

    // The scene faces -X and the hybrid spawned ~4 units that way. Backing up
    // along +X leaves it dead ahead but past the authored 5-unit reach - the
    // case a raycast that ignored the range would silently still drain. Cast
    // immediately: the hybrid aggros on the spawn and closes the gap.
    const start = await game.player.position();
    await game.player.teleport({ x: start.x + 6, y: start.y, z: start.z });
    await game.step({ frames: 2 });

    const before = (await game.info()).player;
    await pullTrigger(game);
    await game.step({ frames: 30 });

    const after = (await game.info()).player;
    assert.equal(after.psi_points, before.psi_points, "an out-of-range drain spends nothing");
    assert.equal(
      await hitPoints(game, monster.id),
      monsterStartHp,
      "an out-of-range drain damages nothing",
    );
  },
);

test(
  "a drain with nothing in its sights changes nothing and spends nothing",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_psi" });

    await game.step({ frames: 10 });
    const monster = await spawnTarget(game);
    const monsterStartHp = await hitPoints(game, monster.id);

    const playerId = (await game.info()).player.entity_id;
    assert.ok(playerId !== null);
    await game.entities.sendMessage(playerId!, { type: "Damage", amount: 20.0 });
    await game.step({ frames: 10 });

    await selectPsiPower(game, "SomaDrain");

    // Turn away from the hybrid: the aim ray now runs into empty scene.
    await game.input.set("head.look", [180, 0]);
    await game.step({ frames: 10 });

    const before = (await game.info()).player;
    await pullTrigger(game);
    await game.step({ frames: 30 });

    const after = (await game.info()).player;
    assert.equal(after.psi_points, before.psi_points, "a whiffed drain spends no psi points");
    assert.equal(after.hit_points, before.hit_points, "a whiffed drain heals nothing");
    assert.equal(
      await hitPoints(game, monster.id),
      monsterStartHp,
      "a whiffed drain damages nothing",
    );
  },
);
