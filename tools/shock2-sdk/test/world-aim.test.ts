import assert from "node:assert/strict";
import test from "node:test";
import {
  AimOcclusionError,
  headRotationForWorldPoint,
  HttpClient,
  PlayerApi,
} from "../src/index.js";
import type {
  EntityDetailResult,
  FrameSnapshot,
  RayCastResult,
} from "../src/index.js";

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
  assert.equal(writes.at(-1)?.path, "/v1/control/input");
});

test("aimAt uses a visible non-creature selectable surface before its center", async () => {
  const detail = {
    entity_id: 470,
    name: "Airlock Door",
    template_id: 470,
    position: [30, -7, 18],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
    aim_points: [],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [30, -8.6, 13],
      rotation: [0, 0, 0, 1],
    },
  } as FrameSnapshot;
  const writes: Array<{ path: string; body: unknown }> = [];
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/470") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string, body: unknown) => {
      writes.push({ path, body });
      if (path === "/v1/physics/raycast") {
        return {
          hit: true,
          hit_point: [30, -7, 17.25],
          hit_normal: [0, 0, -1],
          distance: 4.25,
          entity_id: 470,
          entity_name: "Airlock Door",
          collision_group: "selectable",
          is_sensor: false,
        };
      }
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  const aim = await player.aimAt(470);

  assert.equal(aim.classification, "surface");
  assert.deepEqual(aim.world_point, [30, -7, 17.25]);
  assert.equal(aim.fallback_used, true);
  assert.equal(writes[0]?.path, "/v1/physics/raycast");
  assert.deepEqual(
    (writes[0]?.body as { collision_groups: string[] }).collision_groups,
    ["entity", "selectable", "world", "ui", "raycast"],
  );
  assert.equal(writes[1]?.path, "/v1/control/input");
});

test("aimAt reports center fallback when another entity occludes the target surface", async () => {
  const detail = {
    entity_id: 185,
    name: "Level Transition",
    template_id: 185,
    position: [31.5, -7.9, 17.8],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
    aim_points: [],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [32.34, -8.596, 13.49],
      rotation: [0, 0, 0, 1],
    },
  } as FrameSnapshot;
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/185") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string) => {
      if (path === "/v1/physics/raycast") {
        return {
          hit: true,
          hit_point: [31.8, -7.8, 15.2],
          hit_normal: [0, 0, -1],
          distance: 1.8,
          entity_id: 470,
          entity_name: "Airlock Door",
          collision_group: "entity",
          is_sensor: false,
        };
      }
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  const aim = await player.aimAt(185);

  assert.equal(aim.classification, "center");
  assert.deepEqual(aim.world_point, detail.position);
  assert.equal(aim.fallback_used, true);
});

test("aimAt center remains an explicit origin target without a surface query", async () => {
  const detail = {
    entity_id: 470,
    name: "Airlock Door",
    template_id: 470,
    position: [30, -7, 18],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
    aim_points: [],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [30, -8.6, 13],
      rotation: [0, 0, 0, 1],
    },
  } as FrameSnapshot;
  const writes: string[] = [];
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/470") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string) => {
      writes.push(path);
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  const aim = await player.aimAt(470, { hitbox: "center" });

  assert.equal(aim.classification, "center");
  assert.equal(aim.fallback_used, false);
  assert.deepEqual(writes, ["/v1/control/input"]);
});

test("aimAt visibility required skips an occluded classified proxy", async () => {
  const detail = {
    entity_id: 363,
    name: "Midwife",
    template_id: 352,
    position: [56.7, -14.3, 81.3],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
    aim_points: [
      {
        proxy_entity_id: 1341,
        body_id: 814,
        joint_id: 11,
        classification: "limb",
        position: [56.9, -13.5, 81.5],
      },
      {
        proxy_entity_id: 1344,
        body_id: 815,
        joint_id: 12,
        classification: "limb",
        position: [56.5, -14, 81],
      },
    ],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [62, -15.8, 83],
      rotation: [0, 0, 0, 1],
    },
  } as FrameSnapshot;
  const doorway: RayCastResult = {
    hit: true,
    hit_point: [60.1, -14.4, 82.5],
    hit_normal: [1, 0, 0],
    distance: 2.1,
    entity_id: 77,
    entity_name: "Doorway frame",
    body_id: 501,
    collision_group: "world",
    is_sensor: false,
  };
  const visibleProxy: RayCastResult = {
    hit: true,
    hit_point: detail.aim_points[1].position,
    hit_normal: [1, 0, 0],
    distance: 5.7,
    entity_id: detail.aim_points[1].proxy_entity_id,
    entity_name: null,
    body_id: detail.aim_points[1].body_id,
    collision_group: "hitbox",
    is_sensor: false,
  };
  const raycasts = [doorway, visibleProxy];
  const writes: Array<{ path: string; body: unknown }> = [];
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/363") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string, body: unknown) => {
      writes.push({ path, body });
      if (path === "/v1/physics/raycast") return raycasts.shift();
      return undefined;
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  const aim = await player.aimAt(363, {
    hitbox: "limb",
    visibility: "required",
  });

  assert.equal(aim.proxy_entity_id, 1344);
  assert.equal(aim.fallback_used, false);
  assert.equal(aim.visibility.state, "visible");
  assert.equal(aim.visibility.origin, "view");
  assert.equal(aim.visibility.blocker, null);
  assert.ok(Math.abs(aim.visibility.target_distance - 5.8558) < 0.001);
  assert.equal(
    writes.filter(({ path }) => path === "/v1/physics/raycast").length,
    2,
  );
  assert.equal(
    (
      writes.find(({ path }) => path === "/v1/physics/raycast")
        ?.body as { ignore_sensors: boolean }
    ).ignore_sensors,
    true,
  );
  assert.equal(writes.at(-1)?.path, "/v1/control/input");
});

test("aimAt visibility required reports the blocker when every proxy is occluded", async () => {
  const detail = {
    entity_id: 363,
    name: "Midwife",
    template_id: 352,
    position: [56.7, -14.3, 81.3],
    rotation: [0, 0, 0, 1],
    inheritance_chain: [],
    properties: [],
    outgoing_links: [],
    incoming_links: [],
    aim_points: [
      {
        proxy_entity_id: 1343,
        body_id: 812,
        joint_id: 9,
        classification: "head",
        position: [56.8, -13.2, 81.4],
      },
    ],
  } satisfies EntityDetailResult;
  const snapshot = {
    player: {
      position: [62, -15.8, 83],
      rotation: [0, 0, 0, 1],
    },
  } as FrameSnapshot;
  const doorway: RayCastResult = {
    hit: true,
    hit_point: [60.1, -14.4, 82.5],
    hit_normal: [1, 0, 0],
    distance: 2.1,
    entity_id: 77,
    entity_name: "Doorway frame",
    body_id: 501,
    collision_group: "world",
    is_sensor: false,
  };
  const writes: Array<{ path: string; body: unknown }> = [];
  const client = {
    get: async (path: string) => {
      if (path === "/v1/entities/363") return detail;
      if (path === "/v1/info") return snapshot;
      throw new Error(`unexpected GET ${path}`);
    },
    post: async (path: string, body: unknown) => {
      writes.push({ path, body });
      if (path === "/v1/physics/raycast") return doorway;
      return undefined;
    },
  };
  const player = new PlayerApi(client as unknown as HttpClient);

  await assert.rejects(
    player.aimAt(363, { hitbox: "head", visibility: "required" }),
    (error: unknown) => {
      if (!(error instanceof AimOcclusionError)) return false;
      assert.equal(error.result.proxy_entity_id, 1343);
      assert.equal(error.result.visibility.state, "blocked");
      assert.equal(error.result.visibility.origin, "view");
      assert.ok(
        Math.abs(error.result.visibility.target_distance - 5.5317) < 0.001,
      );
      assert.deepEqual(error.result.visibility.blocker, {
        entity_id: 77,
        entity_name: "Doorway frame",
        body_id: 501,
        collision_group: "world",
        hit_point: [60.1, -14.4, 82.5],
        distance: 2.1,
      });
      return true;
    },
  );
  assert.equal(
    writes.some(({ path }) => path === "/v1/control/input"),
    false,
  );
});
