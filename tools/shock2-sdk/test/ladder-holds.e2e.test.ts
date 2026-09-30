import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, HttpError } from "../src/index.js";
import type { LadderHolds } from "../src/index.js";

// GET /v1/physics/ladder reads a ladder's rungs and rails off its model: its
// physics body is a single box. Rick ladders are two rails with a rung every
// 0.8; the "Ladder 16'" pole ladder is one pole with pegs on alternate sides.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

async function ladderNamed(game: GameServer, name: string): Promise<LadderHolds> {
  const { entities } = await game.entities.list({ filter: "*Ladder*", limit: 400 });
  const entity = entities.find((e) => e.name === name);
  assert.ok(entity, `no entity named ${name}`);
  return game.physics.ladder(entity.id);
}

const round = (v: number) => Math.round(v * 100) / 100;

test(
  "debug_ladder: ladder holds come from each ladder model",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder" });
    await game.step({ frames: 2 });

    // The ledge station's 16' rick ladder, standing on the floor at x = -6.9.
    const grip = (await game.physics.grip([-6.85, 2, 0])).grip;
    assert.equal(grip?.kind, "ladder");
    const rick = await game.physics.ladder(grip!.entity_id!);
    assert.equal(rick.model, "ricklad6");
    assert.deepEqual(rick.rungs.map((r) => round(r[0][1])), [0.4, 1.2, 2, 2.8, 3.6, 4.4, 5.2, 6]);
    assert.deepEqual(rick.rails.map((r) => round(r[0][2])).sort((a, b) => a - b), [-0.4, 0.4]);
    assert.deepEqual(rick.normal.map(round).map(Math.abs), [1, 0, 0]);

    const pole = await ladderNamed(game, "Ladder 16'");
    assert.equal(pole.model, "ladder");
    assert.equal(pole.rungs.length, 6, "six pegs");
    assert.equal(pole.rails.length, 1, "one pole");

    await assert.rejects(
      game.physics.ladder(987_654),
      (error: unknown) => error instanceof HttpError && error.status === 404 && /no entity/.test(error.body),
    );
  },
);

test(
  "eng1: mission ladders report their rungs",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "eng1.mis" });
    await game.step({ frames: 2 });

    const rick = await ladderNamed(game, "Rick Ladder 16");
    assert.equal(rick.rungs.length, 8);
    assert.equal(rick.rails.length, 2);
    const spacing = rick.rungs.slice(1).map((r, i) => round(r[0][1] - rick.rungs[i][0][1]));
    assert.ok(spacing.every((s) => s === 0.8), `rungs every 0.8: ${spacing}`);

    const pole = await ladderNamed(game, "Ladder 16'");
    assert.equal(pole.rungs.length, 6);
    assert.equal(pole.rails.length, 1);
  },
);
