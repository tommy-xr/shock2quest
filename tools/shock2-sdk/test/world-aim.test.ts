import assert from "node:assert/strict";
import test from "node:test";
import {
  headRotationForWorldPoint,
  HttpClient,
  PlayerApi,
} from "../src/index.js";
import type { EntityDetailResult, FrameSnapshot } from "../src/index.js";

type Quat = [number, number, number, number];

function multiplyQuat(a: Quat, b: Quat): Quat {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

test("world aim cancels a non-identity save-restored pawn rotation", () => {
  const pawn: Quat = [0, Math.SQRT1_2, 0, Math.SQRT1_2];
  const head = headRotationForWorldPoint([5, 2, 5], pawn, [-5, 3.6, 5]);
  const composed = multiplyQuat(pawn, head);
  const expected = headRotationForWorldPoint(
    [5, 2, 5],
    [0, 0, 0, 1],
    [-5, 3.6, 5],
  );
  composed.forEach((component, index) => {
    assert.ok(Math.abs(component - expected[index]) < 1e-6);
  });
  assert.notDeepEqual(head, expected, "raw world rotation would aim incorrectly");
});

test("world aim rejects an undefined zero-length direction", () => {
  assert.throws(
    () => headRotationForWorldPoint([1, 2, 3], [0, 0, 0, 1], [1, 3.6, 3]),
    /target must differ/,
  );
});

test("aimAt gracefully falls back when a connected runtime omits aim_points", async () => {
  const detail = {
    entity_id: 531,
    name: "Red Assassin",
    template_id: 254,
    position: [65, -7.5, 10],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [61, -7.5, 10],
      rotation: [0, Math.SQRT1_2, 0, Math.SQRT1_2],
    },
  } as FrameSnapshot;
  const writes: Array<{ path: string; body: unknown }> = [];
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/531") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string, body: unknown) => {
      writes.push({ path, body });
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  const aim = await player.aimAt(531, { hitbox: "torso" });

  assert.equal(aim.entity_id, 531);
  assert.equal(aim.classification, "center");
  assert.equal(aim.fallback_used, true);
  assert.deepEqual(aim.world_point, detail.position);
  assert.equal(writes[0]?.path, "/v1/control/input");
});
