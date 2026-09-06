import assert from "node:assert/strict";
import { test } from "node:test";

import { classifyTrack, tally, type AiSample, type AiTrack } from "../src/ai-reachability.js";

type SampleOverrides = Partial<AiSample>;

/** A track sampled every 0.5s, like the harness does (30 frames at 60Hz). */
function track(samples: SampleOverrides[], overrides: Partial<AiTrack> = {}): AiTrack {
  return {
    entity_id: 1,
    name: "OG-Pipe",
    template_id: 668,
    reachable: true,
    samples: samples.map((s, i) => ({
      t: i * 0.5,
      position: [0, 0, 0],
      yaw: 0,
      distance: 50,
      ...s,
    })),
    ...overrides,
  };
}

/** N samples standing at one spot. */
function stationary(count: number, overrides: SampleOverrides = {}): SampleOverrides[] {
  return Array.from({ length: count }, () => ({ position: [49, 0.1, -98.5] as const, ...overrides }));
}

test("a chasing AI that reaches the player is 'arrived'", () => {
  const result = classifyTrack(
    track([
      { position: [20, 0, 0], distance: 20 },
      { position: [10, 0, 0], distance: 10 },
      { position: [2, 0, 0], distance: 2 },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "arrived");
});

test("a patrolling AI stuck against geometry is 'wedged', with its position", () => {
  // The medsci2 railing bug: patrol route is Full, the AI never moves, and
  // the steering reports stall time.
  const result = classifyTrack(
    track([
      { position: [48, 0.1, -85], outcome: "Full", live_path_len: 4, live_stall_seconds: 0 },
      // The stall clock ticks up and resets on each backout - what a live
      // wedge looks like, as opposed to a frozen snapshot.
      ...stationary(20, { outcome: "Full", live_path_len: 11, behavior: "Patrol" }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
  assert.deepEqual(result.wedge_at, [49, 0.1, -98.5]);
  assert.ok((result.wedge_seconds ?? 0) >= 6);
});

test("standing still with no route is not a wedge", () => {
  const result = classifyTrack(track(stationary(30, { behavior: "Idle" })), { pass: "idle" });
  assert.equal(result.verdict, "idle_by_design");
});

test("a failed route is 'no_route'", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], outcome: "Failed", behavior: "Chase" },
      { position: [1, 0, 0], outcome: "Failed", behavior: "Chase" },
      { position: [3, 0, 0], outcome: "Partial", behavior: "Chase" },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "no_route");
});

test("an AI with no walk route to the player is expected-unreachable, not a failure", () => {
  const result = classifyTrack(
    track(
      [
        { position: [0, 0, 0], outcome: "Failed", behavior: "Chase", alertness: "High" },
        { position: [1, 0, 0], outcome: "Failed", behavior: "Chase", alertness: "High" },
      ],
      { reachable: false },
    ),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "expected_unreachable");
});

test("player connectivity does not judge the idle pass - the AI is on its own patrol", () => {
  const result = classifyTrack(
    track(
      [
        { position: [0, 0, 0], outcome: "Full", behavior: "Patrol" },
        { position: [8, 0, 0], outcome: "Full", behavior: "Patrol" },
      ],
      { reachable: false },
    ),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "progressing");
});

test("an AI that never leaves Lowest under a forced chase is alert-capped", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], alertness: "Lowest", behavior: "Idle" },
      { position: [4, 0, 0], alertness: "Lowest", behavior: "Idle" },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "expected_unreachable");
  assert.match(result.reason, /never rose above Lowest/);
});

test("a scripted-sequence AI is expected-unreachable", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], behavior: "ScriptedSequence", alertness: "High" },
      { position: [5, 0, 0], behavior: "ScriptedSequence", alertness: "High" },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "expected_unreachable");
});

test("an AI still closing on the player is 'progressing'", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 60, outcome: "Full", behavior: "Chase", alertness: "High" },
      { position: [10, 0, 0], distance: 45, outcome: "Full", behavior: "Chase", alertness: "High" },
      { position: [20, 0, 0], distance: 30, outcome: "Full", behavior: "Chase", alertness: "High" },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "progressing");
});

test("a frozen steering snapshot is not evidence of a wedge", () => {
  // The live path/stall record is only refreshed while path-following runs,
  // so an AI that stopped following keeps its last snapshot forever.
  const result = classifyTrack(
    track(stationary(30, { outcome: "Full", live_path_len: 5, live_stall_seconds: 0.78, behavior: "Idle" })),
    { pass: "idle" },
  );
  assert.notEqual(result.verdict, "wedged");
});

test("an AI that starts inside melee reach and never moves is wedged, not 'arrived'", () => {
  const result = classifyTrack(
    track(
      stationary(20, { behavior: "Chase", alertness: "High", live_stall_seconds: 1 }).map((s, i) => ({
        ...s,
        distance: 2.5,
        live_stall_seconds: i % 4, // the stall clock ticks and resets
      })),
    ),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "wedged");
});

test("a wedge that starts after an unblocked stationary spell is still found", () => {
  const result = classifyTrack(
    track([
      // 8s parked with no stall evidence, then 8s parked while stalling.
      ...stationary(16, { behavior: "Patrol" }),
      ...stationary(16, { behavior: "Patrol" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
});

test("a loop patrol that returns to its start is not 'never moved'", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], behavior: "Patrol" },
      { position: [0, 0, 20], behavior: "Patrol" },
      { position: [0, 0, 0], behavior: "Patrol" },
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "progressing");
});

test("tally counts every verdict bucket", () => {
  const counts = tally([
    { verdict: "arrived", reason: "", closest_distance: 1, final_distance: 1 },
    { verdict: "wedged", reason: "", closest_distance: 1, final_distance: 1 },
    { verdict: "wedged", reason: "", closest_distance: 1, final_distance: 1 },
  ]);
  assert.equal(counts.arrived, 1);
  assert.equal(counts.wedged, 2);
  assert.equal(counts.no_route, 0);
});
