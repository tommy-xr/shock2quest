import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end test for the non-finite body-state DETECTION layer (#506).
//
// A rigid body whose velocity goes non-finite integrates into a non-finite
// pose, which puts a NaN AABB into rapier's broad-phase BVH - and parry's
// binned rebuild (bvh_binned_build.rs) then panics with "index out of
// bounds" a few frames later, killing the main thread mid-walk (5/5
// campaign crashes; upstream: dimforge/rapier#961, unfixed as of parry
// 0.29).
//
// This layer does NOT prevent that crash - it attributes it:
// PhysicsWorld::report_nonfinite_rigid_body_state logs an ERROR naming the
// exact entity and offending field(s) at the first bad frame, once per
// entity, without mutating any state. The producer-side fix (NaN animation
// joints driving kinematic hitboxes) is #508.
//
// The poison is injected through the real debug surface: two opposite
// overflow-to-infinity impulses on one dynamic body (inf + -inf = NaN
// linvel). The test steps a single frame - enough for one update to run the
// reporter, and safely before the (still-inevitable) parry panic, which
// needs the poisoned pose to reach a broad-phase rebuild - then asserts the
// report fired and the body state was left untouched (still non-finite).
const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "station.mis: non-finite body state is attributed to its entity in the log (#506)",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "station.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8340),
    });
    await game.step({ frames: 30 }); // settle

    // Baseline: the organic #508 producer (NaN hitbox poses) may already
    // have reported at load, but a *velocity* report can only come from our
    // poison below.
    const linvelReports = () =>
      game
        .logs()
        .filter(
          (l) => l.includes("non-finite rigid-body state") && l.includes("linvel"),
        );
    assert.equal(
      linvelReports().length,
      0,
      "no non-finite velocity report should fire before the poison",
    );

    // Poison every dynamic body (ids are not stable across runs, so discover
    // them each launch). Animated creatures have their velocity overwritten
    // by animation locomotion each frame, which would silently clear the
    // poison - but non-animated items (e.g. the starter pickups) keep it.
    // 1e300 overflows f32 to +inf; the pair sums to NaN in the body's linvel.
    const { bodies } = await game.physics.bodies({ limit: 500 });
    const dynamics = bodies.filter((b) => b.body_type === "dynamic");
    assert.ok(dynamics.length > 0, "station.mis should have dynamic bodies");
    for (const body of dynamics) {
      for (const x of [1e300, -1e300]) {
        const res = await fetch(
          `${game.baseUrl}/v1/physics/bodies/${body.body_id}/impulse`,
          { method: "POST", body: JSON.stringify({ impulse: [x, 0, 0] }) },
        );
        const result = (await res.json()) as { success: boolean };
        assert.ok(result.success, `impulse ${x} on body ${body.body_id} should apply`);
      }
    }

    // One frame: the reporter runs at the top of the physics update.
    await game.step({ frames: 1 });

    // The report names the poisoned entity and the offending field. The log
    // line is written by the game thread; the SDK reads it off a pipe, so
    // poll briefly rather than racing the flush.
    await game.waitFor(() => linvelReports().length > 0, {
      timeoutMs: 5_000,
      description: "a non-finite state report naming linvel",
    });
    const reports = linvelReports();
    assert.ok(
      reports.some((l) => l.includes("ERROR")),
      `the report should be ERROR level, got: ${reports[0]}`,
    );

    // Detection only - the poisoned state must NOT have been repaired: at
    // least one poisoned body (a non-animated one) must still be non-finite.
    // (serde_json serializes NaN/inf as null, so a non-finite component
    // surfaces as a non-finite/null entry here.)
    const after = await game.physics.bodies({ limit: 500 });
    const poisonedIds = new Set(dynamics.map((b) => b.body_id));
    const stillBad = after.bodies.filter(
      (b) =>
        poisonedIds.has(b.body_id) &&
        ![...b.velocity, ...b.position].every((c) => Number.isFinite(c)),
    );
    assert.ok(
      stillBad.length > 0,
      "detection must not mutate state - at least one poisoned body should still be non-finite",
    );
  },
);
