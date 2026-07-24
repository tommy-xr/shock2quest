import assert from "node:assert/strict";

import type { GameServer } from "../../src/index.js";

/**
 * Cross one authored Earth training tripwire through a bounded,
 * collision-valid move and verify its linked teleport destination fired.
 *
 * Teleports only stage outside each cardinal face of the sensor. The actual
 * interaction is Rapier's production SensorBeginIntersect path.
 */
export async function crossEarthTrainingTripwire(
  game: GameServer,
  tripwireTemplate: number,
  destinationTemplate: number,
): Promise<void> {
  const tripwires = await game.entities.byTemplate(tripwireTemplate);
  assert.equal(
    tripwires.length,
    1,
    `expected exactly one Earth training tripwire ${tripwireTemplate}`,
  );
  const destinations = await game.entities.byTemplate(destinationTemplate);
  assert.equal(
    destinations.length,
    1,
    `expected exactly one Earth training destination ${destinationTemplate}`,
  );
  const [tripwire] = tripwires;
  const [destination] = destinations;
  const [x, y, z] = tripwire.position;
  const [dx, , dz] = destination.position;

  // Training rooms wall off different faces of their sensors. Try each
  // cardinal approach, starting outside and entering the sensor under
  // collision. The linked teleport proves the real tripwire fired.
  const approaches = [
    { x, y: y + 0.5, z: z - 4 },
    { x: x - 4, y: y + 0.5, z },
    { x: x + 4, y: y + 0.5, z },
    { x, y: y + 0.5, z: z + 4 },
  ];
  for (const start of approaches) {
    await game.player.teleport(start);
    const beforeMove = await game.player.position();
    assert.ok(
      Math.hypot(beforeMove.x - dx, beforeMove.z - dz) >= 3,
      "spatial setup must not teleport through the tripwire",
    );
    await game.step({ frames: 3 });
    const afterSetup = await game.player.position();
    assert.ok(
      Math.hypot(afterSetup.x - dx, afterSetup.z - dz) >= 3,
      "spatial setup must remain outside the tripwire sensor",
    );
    const moved = await game.player.moveTo({ x, y: y + 0.5, z });
    await game.step({ frames: 12 });
    const arrived = await game.player.position();
    if (moved.moved && Math.hypot(arrived.x - dx, arrived.z - dz) < 3) {
      return;
    }
  }

  const arrived = await game.player.position();
  assert.fail(
    `tripwire ${tripwireTemplate} should fire its authored teleport to ${destinationTemplate}; ` +
      `destination=${JSON.stringify(destination.position)} actual=${JSON.stringify(arrived)}`,
  );
}
