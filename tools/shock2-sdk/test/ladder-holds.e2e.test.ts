import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer, HttpError, vrGrab } from "../src/index.js";
import type { LadderHolds, Vec3 } from "../src/index.js";
import { add, dot, scale, sub } from "../src/vec.js";
import { teleportVerified } from "./helpers/teleport.js";

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

/** The point on segment [a, b] nearest `p`. */
function nearestOn([a, b]: [Vec3, Vec3], p: Vec3): Vec3 {
  const along = sub(b, a);
  const t = Math.min(1, Math.max(0, dot(sub(p, a), along) / dot(along, along)));
  return add(a, scale(along, t));
}

const mid = ([a, b]: [Vec3, Vec3]): Vec3 => scale(add(a, b), 0.5);
const distance = (a: Vec3, b: Vec3) => Math.sqrt(dot(sub(a, b), sub(a, b)));

/**
 * Stand in front of `ladder` and close the right hand on each case: `on`
 * holds must grip the `expected` member, snapped onto it; `off` points (the
 * ladder's face, but no member within reach) must grip nothing.
 */
async function assertHolds(
  game: GameServer,
  ladder: LadderHolds,
  on: { point: Vec3; member: [Vec3, Vec3] }[],
  off: Vec3[],
) {
  for (const point of off) {
    await assert.rejects(vrGrab(game, "right", point), /closed on nothing/, `no hold at ${point}`);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
  }
  for (const { point, member } of on) {
    const hold = await vrGrab(game, "right", point);
    assert.equal(hold.kind, "ladder");
    assert.equal(hold.entity_id, ladder.entity_id);
    const snapped = nearestOn(member, point);
    assert.ok(distance(hold.point, snapped) < 0.01, `held ${hold.point}, expected ${snapped}`);
    await game.input.set("right_hand.squeeze", 0);
    await game.step({ frames: 2 });
  }
}

test(
  "debug_ladder (VR): a hand holds a ladder only on its rungs and rails, snapped onto them",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_ladder", debugFlags: ["--vr"] });
    await game.step({ frames: 5 });

    // The ledge station's rick ladder: x = -6.9 face, rungs every 0.8 from
    // 0.4, rails at z = ±0.4. The hand closes 0.1 in front of the face.
    await teleportVerified(game, { x: -6.48, y: 1.5, z: 0 });
    await game.step({ frames: 30 });
    const rick = await game.physics.ladder((await game.physics.grip([-6.8, 2.8, 0])).grip!.entity_id!);
    const rung = rick.rungs[2];
    const rail = rick.rails.find(([bottom]) => bottom[2] > 0)!;
    await assertHolds(
      game,
      rick,
      [
        { point: [-6.8, 2.05, 0.1], member: rung },
        { point: [-6.8, 2.2, 0.36], member: rail },
      ],
      [
        [-6.8, 2.4, 0],
        [-6.8, 1.6, 0.1],
      ],
    );

    // The trench station's eng1 pole ladder: a pole with a peg every ~1.07,
    // alternating sides. Off a peg's end, half a spacing up, is bare face.
    const pole = await ladderNamed(game, "Ladder 16'");
    const [shaft] = pole.rails;
    const toward = scale(pole.normal, Math.sign(pole.normal[0]));
    const front = (p: Vec3) => add(p, scale(toward, 0.1));
    const peg = pole.rungs.reduce((best, r) => (Math.abs(mid(r)[1] - 2) < Math.abs(mid(best)[1] - 2) ? r : best));
    // In front of the peg, so it and the pole are both in arm's reach.
    const stand = add(mid(peg), scale(toward, 0.5));
    await teleportVerified(game, { x: stand[0], y: 1.5, z: stand[2] });
    await game.step({ frames: 30 });
    await assertHolds(
      game,
      pole,
      [
        { point: front(mid(peg)), member: peg },
        { point: front([shaft[0][0], 2.2, shaft[0][2]]), member: shaft },
      ],
      [front(add(mid(peg), [0, 0.5, 0]))],
    );
  },
);
