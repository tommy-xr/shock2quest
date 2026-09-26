import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

// What the HUD highlight frames, as reported by /v1/entities/:id. A creature's
// own collider is a standing capsule sized from its creature definition, so it
// frames a nominal cylinder rather than the creature - and says the same thing
// whatever the creature is doing. The highlight now frames the union of its
// hitboxes: the volume its limbs occupy in the pose it is in, and the same
// volume the creature is shot by.
test(
  "a creature's highlight frames its hitboxes; a corpse keeps its collider",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "debug_melee" });
    await game.step({ frames: 40 });

    // Hitboxes are the identifying feature here, so find the creature by
    // them rather than by a scene-local name.
    const { entities } = await game.entities.list();
    let detail;
    for (const entity of entities) {
      const candidate = await game.entities.detail(entity.id);
      if ((candidate.aim_points ?? []).length > 0) {
        detail = candidate;
        break;
      }
    }
    assert.ok(detail, "expected a creature with hitboxes in debug_melee");
    const bounds = detail.selection_bounds;
    assert.ok(bounds, "a creature should report selection bounds");

    // Negative-first: the standing capsule does not contain the hitboxes of an
    // animated creature (arms out, weapon raised), so this fails on the
    // collider bounds it used to report.
    const [min, max] = bounds;
    const points = detail.aim_points ?? [];
    for (const point of points) {
      for (let axis = 0; axis < 3; axis++) {
        assert.ok(
          point.position[axis] >= min[axis] - 0.01 &&
            point.position[axis] <= max[axis] + 0.01,
          `hitbox ${point.joint_id} (${point.classification}) sits outside the highlight on axis ${axis}: ${JSON.stringify(point.position)} vs ${JSON.stringify(bounds)}`,
        );
      }
    }

    // ...and it frames THIS creature, not the room: the box may exceed the
    // hitbox centres only by about a limb's thickness. (Containment alone
    // cannot fail - the bounds are the union of these very proxies - so this
    // is the half of the check that can.)
    for (let axis = 0; axis < 3; axis++) {
      const lo = Math.min(...points.map((point) => point.position[axis]));
      const hi = Math.max(...points.map((point) => point.position[axis]));
      assert.ok(
        min[axis] > lo - 0.8 && max[axis] < hi + 0.8,
        `the highlight should hug the hitboxes on axis ${axis}: hitboxes span ${lo.toFixed(2)}..${hi.toFixed(2)}, box spans ${min[axis].toFixed(2)}..${max[axis].toFixed(2)}`,
      );
    }
  },
);

test(
  "an authored corpse has no hitboxes, so its highlight keeps its collider",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    const corpse = (await game.entities.list()).entities
      .filter((entity) => entity.name === "MS Male Corpse")
      .sort((a, b) => a.distance - b.distance)[0];
    assert.ok(corpse, "expected a corpse in medsci1");
    const detail = await game.entities.detail(corpse.id);
    assert.equal((detail.aim_points ?? []).length, 0, "a posed corpse has no hitbox proxies");

    const bounds = detail.selection_bounds;
    assert.ok(bounds, "the corpse should still report bounds - its collider");
    const [min, max] = bounds;
    // The PR-1 body-shaped box: a body length across, and flat.
    assert.ok(max[0] - min[0] > 2, `expected a body-length box, got ${JSON.stringify(bounds)}`);
    assert.ok(max[1] - min[1] < 1.2, `expected a flat box, got ${JSON.stringify(bounds)}`);
  },
);

test(
  "an arachnid is framed by its legs, not just its body",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "hydro3.mis" });
    await game.step({ frames: 60 });

    const arachnid = (await game.entities.list()).entities.find((entity) =>
      entity.name.includes("Arachnid"),
    );
    assert.ok(arachnid, "expected a Baby Arachnid in hydro3");
    const detail = await game.entities.detail(arachnid.id);
    const points = detail.aim_points ?? [];
    assert.equal(
      points.length,
      21,
      "the arachnid definition maps every skinned joint: body, mandibles, eight legs",
    );
    const parts = new Set(points.map((point) => point.classification));
    for (const part of ["torso", "head", "limb", "extremity"] as const) {
      assert.ok(parts.has(part), `expected a ${part} hitbox on the arachnid`);
    }

    // With only the Body joint mapped the frame measured 0.33-0.58 across -
    // the body blob, legs excluded. The legs roughly double that.
    const bounds = detail.selection_bounds;
    assert.ok(bounds, "the arachnid should report bounds");
    const width = Math.max(bounds[1][0] - bounds[0][0], bounds[1][2] - bounds[0][2]);
    const height = bounds[1][1] - bounds[0][1];
    assert.ok(
      width > 0.8 && width < 2.0,
      `expected a leg-span frame, got ${width.toFixed(2)} across`,
    );
    assert.ok(height > 0.25, `expected a body-scale frame, got ${height.toFixed(2)} tall`);
  },
);
