import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type {
  EntityDetailResult,
  EntitySummary,
  PhysicsBodySummary,
} from "../src/types.js";

// Command's two objective gates use the same authored pattern:
//
//   tripwire 77/606 -> locked card slot 621 -> umbilical door 251
//   tripwire 70/74  -> locked card slot 69  -> shuttle-bay doors 476/477
//
// The slots have PropLocked(true), no key destination, and TweqLockedButton.
// A tripwire TurnOn must therefore stop at the locked button instead of
// relaying to the doors and bypassing their authored quest-bit openers (#664).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const UMBILICAL_DOOR = 251;
const UMBILICAL_TRIPWIRE = 77;
const SHUTTLE_DOORS = [476, 477] as const;
const SHUTTLE_TRIPWIRE = 70;

async function only(game: GameServer, objectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one command1 object ${objectId}, got ${JSON.stringify(found.map((e) => e.name))}`,
  );
  return found[0];
}

async function doorState(
  game: GameServer,
  objectId: number,
): Promise<{ detail: EntityDetailResult; body: PhysicsBodySummary }> {
  const door = await only(game, objectId);
  const [body] = (await game.physics.bodies({ entityId: door.id })).bodies;
  assert.ok(body, `door ${objectId} must own a collider`);
  assert.equal(body.body_type, "kinematic", `door ${objectId} collider must be kinematic`);
  assert.equal(body.is_enabled, true, `door ${objectId} collider must be enabled`);
  return {
    detail: await game.entities.detail(door.id),
    body,
  };
}

function distance(a: readonly number[], b: readonly number[]): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

async function enterTripwire(game: GameServer, objectId: number): Promise<void> {
  const tripwire = await only(game, objectId);
  const [x, y, z] = tripwire.position;
  await game.player.teleport({ x, y: y + 0.5, z });
  await game.step({ frames: 240 });
}

test(
  "command1: locked card slots do not relay tripwire TurnOn to quest-gated doors",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "command1.mis",
    });
    await game.step({ frames: 5 });

    assert.equal(await game.quests.get("ShuttleABoom"), "unknown");
    assert.equal(await game.quests.get("ShuttleBBoom"), "unknown");
    assert.equal(await game.quests.get("opscom"), "unknown");
    assert.equal(await game.quests.get("engcom"), "unknown");

    const umbilicalBefore = await doorState(game, UMBILICAL_DOOR);
    const shuttleBefore = await Promise.all(
      SHUTTLE_DOORS.map((door) => doorState(game, door)),
    );

    // Establish that door 251 starts physically shut, rather than merely
    // looking displaced: it owns an enabled kinematic collider at the same
    // live position as the rendered entity. (A Swarmer Floor Pod overlaps the
    // approach ray here, so body inspection is the unambiguous check.)
    assert.ok(
      distance(umbilicalBefore.detail.position, umbilicalBefore.body.position) < 0.01,
      "umbilical door collider must start at the rendered closed position",
    );

    // Enter the live authored tripwires without frobbing anything. Teleport is
    // staged as locomotion and reaches TrapNewTripwire through Rapier's real
    // SensorBeginIntersect message.
    await enterTripwire(game, UMBILICAL_TRIPWIRE);
    await enterTripwire(game, SHUTTLE_TRIPWIRE);

    const umbilicalAfter = await doorState(game, UMBILICAL_DOOR);
    const shuttleAfter = await Promise.all(
      SHUTTLE_DOORS.map((door) => doorState(game, door)),
    );

    assert.ok(
      distance(umbilicalAfter.detail.position, umbilicalAfter.body.position) < 0.01,
      "door 251's collider must follow its live rendered position",
    );
    for (const [index, objectId] of SHUTTLE_DOORS.entries()) {
      assert.ok(
        distance(shuttleAfter[index].detail.position, shuttleAfter[index].body.position) < 0.01,
        `door ${objectId}'s collider must follow its live rendered position`,
      );
    }

    const changedDoors = [
      {
        objectId: UMBILICAL_DOOR,
        before: umbilicalBefore.detail.position,
        after: umbilicalAfter.detail.position,
        bodyAfter: umbilicalAfter.body.position,
      },
      ...SHUTTLE_DOORS.map((objectId, index) => ({
        objectId,
        before: shuttleBefore[index].detail.position,
        after: shuttleAfter[index].detail.position,
        bodyAfter: shuttleAfter[index].body.position,
      })),
    ].filter(({ before, after }) => distance(before, after) >= 0.01);
    assert.deepEqual(
      changedDoors,
      [],
      `locked card slots must leave every quest-gated door closed: ${JSON.stringify(changedDoors)}`,
    );
  },
);
