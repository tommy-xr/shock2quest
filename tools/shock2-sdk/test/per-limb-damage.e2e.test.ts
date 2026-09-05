import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// What a blow is worth depends on where it landed: a head is worth more than a
// shin, and a creature's own weapon is worth nothing. This is what the per-joint
// hitboxes are *for* - aiming only matters if aim changes the outcome.
//
// Each blow lands on a different creature: 10 points is enough to kill, and a
// dead creature ignores damage, so reusing one target would read every blow
// after the first as zero.
const AUTHORED = 10;

async function hitPoints(game: GameServer, entityId: number): Promise<number | null> {
  const detail = await game.entities.detail(entityId);
  const property = detail.properties.find((p) => p.name === "HitPoints");
  return property ? Number(property.value) : null;
}

test(
  "a blow is worth what the part it struck is worth",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 30 });

    // The joint table below is the humanoid one, so pick the hybrids by name:
    // the scene's arachnids have per-joint hitboxes of their own now.
    const creatures = [];
    for (const entity of (await game.entities.list({ filter: "OG-Pipe" })).entities) {
      const detail = await game.entities.detail(entity.id);
      if ((detail.aim_points ?? []).length > 1) creatures.push(detail);
    }
    assert.ok(creatures.length >= 3, "expected three hybrids in debug_melee");

    // joint -> the factor the hitbox table gives it.
    const cases: Array<[number, string, number]> = [
      [9, "head", 1.25],
      [18, "abdomen", 1.0],
      [12, "elbow", 0.5],
    ];

    for (const [index, [joint, part, factor]] of cases.entries()) {
      const creature = creatures[index]!;
      const aim = (creature.aim_points ?? []).find((point) => point.joint_id === joint);
      assert.ok(aim, `expected a ${part} hitbox on the creature`);

      const before = await hitPoints(game, creature.entity_id);
      assert.ok(before !== null, "the creature should report hit points");
      await game.entities.sendMessage(aim.proxy_entity_id, {
        type: "Damage",
        amount: AUTHORED,
        point: aim.position,
        direction: [0, 0, 1],
      });
      await game.step({ frames: 3 });
      const after = await hitPoints(game, creature.entity_id);
      assert.ok(after !== null, "the creature should still report hit points");

      // Negative-first: unscaled, every part costs the authored 10.
      const dealt = before - after;
      assert.ok(
        Math.abs(dealt - AUTHORED * factor) <= 0.5,
        `a blow on the ${part} should cost about ${AUTHORED * factor}, got ${dealt}`,
      );
    }
  },
);

test(
  "a blow on the hand is worth a far-limb blow, and still reaches the creature",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 30 });

    // Joints 14/15 are `LWeap`/`RWeap`: where a weapon is *attached*. The
    // vertices skinned to them are the creature's own hand - the pipe it
    // carries is a separate object with no hitbox at all - so a blow there is
    // a blow on a hand, worth what the far half of a limb is worth.
    let creature;
    for (const entity of (await game.entities.list({ filter: "OG-Pipe" })).entities) {
      const detail = await game.entities.detail(entity.id);
      if ((detail.aim_points ?? []).some((point) => point.joint_id === 14 || point.joint_id === 15)) {
        creature = detail;
        break;
      }
    }
    assert.ok(creature, "expected a creature with weapon-hand hitboxes");
    const hand = (creature.aim_points ?? []).find(
      (point) => point.joint_id === 14 || point.joint_id === 15,
    )!;

    const before = await hitPoints(game, creature.entity_id);
    assert.ok(before !== null, "the creature should report hit points");
    await game.entities.sendMessage(hand.proxy_entity_id, {
      type: "Damage",
      amount: AUTHORED,
      point: hand.position,
      direction: [0, 0, 1],
    });
    await game.step({ frames: 3 });
    const after = await hitPoints(game, creature.entity_id);
    assert.ok(after !== null, "the creature should still report hit points");

    const dealt = before - after;
    assert.ok(
      Math.abs(dealt - AUTHORED * 0.5) <= 0.5,
      `a blow on the hand should cost about ${AUTHORED * 0.5}, got ${dealt}`,
    );
  },
);
