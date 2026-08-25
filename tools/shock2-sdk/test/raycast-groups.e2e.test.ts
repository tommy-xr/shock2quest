import assert from "node:assert/strict";
import { test } from "node:test";

import {
  GameServer,
  HttpClient,
  HttpError,
  type RayCastResult,
} from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

const wallRay = {
  start: [79.99, -1.5, 32.0] as [number, number, number],
  end: [73.99, -1.96, 32.0] as [number, number, number],
  ignore_sensors: true,
};

test(
  "raycast defaults ignore sensors while groups remain validated and compatible",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
    });
    // The first physics update populates Rapier's query broad phase.
    await game.step({ frames: 1 });

    // `level` was advertised by the API and used by its old default. Keep it
    // as a compatibility alias, but resolve it to the real `world` group.
    const aliasHit = await game.raycast({
      ...wallRay,
      collision_groups: ["level"],
    });
    assert.equal(aliasHit.hit, true, "level alias should see Hydro2 world geometry");
    assert.ok(
      aliasHit.distance !== null && Math.abs(aliasHit.distance - 1.5947) < 0.01,
      `expected the Hydro2 wall about 1.595 units away, got ${JSON.stringify(aliasHit)}`,
    );

    const client = new HttpClient(game.baseUrl);
    await assert.rejects(
      client.post("/v1/physics/raycast", {
        ...wallRay,
        collision_groups: ["bogus"],
      }),
      (error: unknown) =>
        error instanceof HttpError &&
        error.status === 400 &&
        /unknown collision group 'bogus'/i.test(error.body),
      "unknown group should receive a clear 400 response",
    );
    await assert.rejects(
      game.raycast({ ...wallRay, collision_groups: [] }),
      (error: unknown) =>
        error instanceof HttpError &&
        error.status === 400 &&
        /collision_groups must contain at least one group/i.test(error.body),
      "an explicit empty mask should receive a clear 400 response",
    );

    // Omission remains the convenient common case and must ignore room-trigger
    // sensors. This origin is inside a Base Room sensor; the wall is the first
    // solid surface along the ray.
    const defaultHit = await game.raycast({
      start: [79.0, -1.6, 32.0],
      end: wallRay.end,
    });
    assert.equal(defaultHit.hit, true, "default raycast should see the Hydro2 wall");
    assert.ok(
      defaultHit.distance !== null && Math.abs(defaultHit.distance - 0.6015) < 0.01,
      `expected the nearby Hydro2 wall, got ${JSON.stringify(defaultHit)}`,
    );

    // Raw HTTP callers can still opt in when they are deliberately probing
    // trigger volumes.
    const sensorHit = await client.post<RayCastResult>("/v1/physics/raycast", {
      start: [79.0, -1.6, 32.0],
      end: wallRay.end,
      ignore_sensors: false,
    });
    assert.equal(sensorHit.hit, true);
    assert.equal(sensorHit.is_sensor, true);
    assert.equal(sensorHit.entity_name, "Base Room");
    assert.equal(sensorHit.distance, 0);
  },
);
