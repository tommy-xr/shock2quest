import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, Vec3 } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// This medsci1 barrel sits beside the solid x=-3.2 corridor wall, with
// walkable floor on both sides. Runtime ids change every launch; its mission
// template backlink is stable.
const COVER_BARREL_TEMPLATE_ID = 872;
const PLAYER_HEIGHT_ABOVE_BARREL = 1.09;
const PLAYER_HORIZONTAL_OFFSET = 3.0;

async function coverBarrel(game: GameServer): Promise<EntitySummary> {
  const { entities } = await game.entities.list({ filter: "Explode Barrel", limit: 20 });
  const barrel = entities.find((entity) => entity.template_id === COVER_BARREL_TEMPLATE_ID);
  assert.ok(barrel, `expected barrel template ${COVER_BARREL_TEMPLATE_ID}`);
  return barrel;
}

async function detonateBesidePlayer(
  playerSide: "covered" | "uncovered",
  port: number,
): Promise<{ damage: number; lineBlocked: boolean }> {
  await using game = await GameServer.launch({ mission: "medsci1.mis", port });
  await game.step({ frames: 2 });

  const barrel = await coverBarrel(game);
  const [bx, by, bz] = barrel.position;
  const playerPosition: Vec3 = [
    bx + (playerSide === "covered" ? -PLAYER_HORIZONTAL_OFFSET : PLAYER_HORIZONTAL_OFFSET),
    by + PLAYER_HEIGHT_ABOVE_BARREL,
    bz,
  ];
  await game.player.teleport({
    x: playerPosition[0],
    y: playerPosition[1],
    z: playerPosition[2],
  });
  await game.step({ frames: 2 });

  const settledPlayer = await game.info();
  const hitPointsBefore = settledPlayer.player.hit_points;
  assert.ok(hitPointsBefore !== null, "player should have a health pool");
  const line = await game.raycast({
    start: barrel.position,
    end: settledPlayer.player.position,
    collision_groups: ["world"],
  });

  await game.entities.sendMessage(barrel.id, { type: "Damage", amount: 5.0 });
  await game.step({ frames: 8 });
  const hitPointsAfter = (await game.info()).player.hit_points;
  assert.ok(hitPointsAfter !== null, "player should retain a health pool");

  return {
    damage: hitPointsBefore - hitPointsAfter,
    lineBlocked: line.hit,
  };
}

test(
  "Explosion occlusion: a corridor wall shields the player while open space does not",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const covered = await detonateBesidePlayer(
      "covered",
      Number(process.env.SHOCK2_E2E_PORT ?? 8144),
    );
    assert.ok(covered.lineBlocked, "the covered control ray must hit the corridor wall");
    assert.equal(covered.damage, 0, "the wall must absorb the whole blast");

    const uncovered = await detonateBesidePlayer(
      "uncovered",
      Number(process.env.SHOCK2_E2E_CONTROL_PORT ?? 8145),
    );
    assert.ok(!uncovered.lineBlocked, "the open-space control ray must stay clear");
    assert.ok(uncovered.damage > 0, "the unobstructed blast must still hurt the player");
  },
);
