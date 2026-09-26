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

test("an AI with no route to the player is not a wedge, even while it stalls", () => {
  // A stall clock that ticks under a Failed path is a routing failure, not a
  // physical wedge - `no_route` outranks `wedged`.
  const result = classifyTrack(
    track(
      stationary(20, { behavior: "Chase", alertness: "High", outcome: "Failed" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
      })),
    ),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "no_route");
});

test("a Partial outside the halt does not outrank a wedge that happened with a route", () => {
  // Path outcomes are sticky: an early Partial, and a late Partial published
  // once the AI shuffles and re-queries, both sit outside the halt itself -
  // which the AI spent with a Full route in hand and never got clear of.
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 50, behavior: "Chase", outcome: "Partial" },
      { position: [4, 0, 0], distance: 46, behavior: "Chase", outcome: "Full" },
      ...stationary(18, { behavior: "Chase", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
      // Shuffles 2 units - never gets clear (the freed threshold is 5).
      { position: [51, 0.1, -98.5], distance: 50, behavior: "Chase", outcome: "Partial" },
      { position: [51, 0.1, -98.5], distance: 50, behavior: "Chase", outcome: "Partial" },
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
  assert.ok((result.wedge_seconds ?? 0) >= 6);
  assert.equal(result.halts?.[0].outcome, "Full");
});

test("a Partial published during a hold after the halt does not rewrite it", () => {
  // The halt being judged is the unheld stretch; a route re-query while the
  // AI waits on a door afterwards belongs to the wait, not to the halt.
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 50, behavior: "Patrol", outcome: "Full" },
      ...stationary(18, { behavior: "Patrol", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
      ...stationary(6, {
        behavior: "Patrol",
        outcome: "Partial",
        movement_hold: "DoorWait",
        live_stall_seconds: 1,
      }),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
  assert.equal(result.halts?.[0].outcome, "Full");
  assert.ok((result.halts?.[0].window_held_seconds ?? 0) > 0);
});

test("halt evidence survives a non-wedge verdict", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 50, behavior: "Chase" },
      ...stationary(16, { behavior: "Chase", outcome: "Full", distance: 50 }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
      { position: [1, 0, 0], distance: 1, behavior: "Chase", outcome: "Full" },
    ]),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "arrived");
  assert.equal(result.halts?.length, 1);
});

test("a Partial during a halt the AI walked out of does not make it 'no_route'", () => {
  // It left, so it plainly had somewhere to go: the halt it escaped does not
  // get to decide the verdict, and pass-end is what answers "no route".
  const result = classifyTrack(
    track([
      ...stationary(16, { behavior: "Patrol", outcome: "Partial" }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
      ...Array.from({ length: 12 }, (_, i) => ({
        position: [49 + (i + 1) * 5, 0.1, -98.5] as [number, number, number],
        behavior: "Patrol",
        outcome: "Full",
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged_then_freed");
  assert.equal(result.halts?.[0].outcome, "Partial");
});

test("a halt that happens while the route is Partial is still 'no_route'", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 50, behavior: "Chase", outcome: "Full" },
      ...stationary(20, { behavior: "Chase", outcome: "Partial" }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "no_route");
});

test("a classified halt carries its raw evidence", () => {
  const result = classifyTrack(
    track([
      { position: [0, 0, 0], distance: 50, behavior: "Patrol", outcome: "Full" },
      ...stationary(16, { behavior: "Patrol", outcome: "Full", movement_hold: null }).map((s, i) => ({
        ...s,
        live_stall_seconds: (i % 6) * 0.5,
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
  const halts = result.halts ?? [];
  assert.equal(halts.length, 1);
  assert.equal(halts[0].outcome, "Full");
  assert.equal(halts[0].window_held_seconds, 0);
  assert.deepEqual(halts[0].at, [49, 0.1, -98.5]);
  assert.ok(halts[0].start_t >= 0.5);
  assert.ok(halts[0].seconds >= 6);
  assert.equal(halts[0].escape_distance, 0);
});

test("an AI that was never coming is expected-unreachable, not a wedge", () => {
  const result = classifyTrack(
    track(
      stationary(20, { behavior: "Chase", alertness: "High", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
      })),
      { reachable: false },
    ),
    { pass: "chase" },
  );
  assert.equal(result.verdict, "expected_unreachable");
});

test("a halt the AI walks out of is 'wedged_then_freed', not 'wedged'", () => {
  const result = classifyTrack(
    track([
      ...stationary(20, { behavior: "Patrol", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
      })),
      // Walks 30 units away over the next 3 samples.
      { position: [59, 0.1, -98.5], behavior: "Patrol", outcome: "Full" },
      { position: [69, 0.1, -98.5], behavior: "Patrol", outcome: "Full" },
      { position: [79, 0.1, -98.5], behavior: "Patrol", outcome: "Full" },
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged_then_freed");
  assert.ok((result.freed_travel ?? 0) > 5);
});

test("oscillating in place after a halt is still 'wedged', not freed", () => {
  // Path length racks up, but the AI never gets clear of the pocket.
  const result = classifyTrack(
    track([
      ...stationary(16, { behavior: "Patrol", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
      })),
      ...Array.from({ length: 20 }, (_, i) => ({
        position: [49 + (i % 2) * 1.1, 0.1, -98.5] as [number, number, number],
        behavior: "Patrol",
        outcome: "Full",
      })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
});

test("an AI that walks out of one halt and into a real one is 'wedged'", () => {
  const held = (i: number) => ({ live_stall_seconds: i % 6, behavior: "Patrol", outcome: "Full" });
  const result = classifyTrack(
    track([
      ...Array.from({ length: 16 }, (_, i) => ({ position: [0, 0, 0] as [number, number, number], ...held(i) })),
      { position: [10, 0, 0], ...held(1) },
      { position: [20, 0, 0], ...held(2) },
      ...Array.from({ length: 16 }, (_, i) => ({ position: [30, 0, 0] as [number, number, number], ...held(i) })),
    ]),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
  assert.deepEqual(result.wedge_at, [30, 0, 0]);
});

test("time spent under a movement hold does not count toward the wedge window", () => {
  // 10s parked, 8s of it waiting on a door: only 2s of unheld stall, so the
  // 6s window is never met.
  const result = classifyTrack(
    track(
      stationary(20, { behavior: "Chase", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
        movement_hold: i < 16 ? "DoorWait" : null,
      })),
    ),
    { pass: "idle" },
  );
  assert.notEqual(result.verdict, "wedged");
});

test("short unheld pauses on either side of a long hold do not add up to a wedge", () => {
  // 3.5s stalled, a 30s door wait, 3.5s stalled: 7s unheld in total, but no
  // 6s stretch of it - and the whole thing is one stationary span.
  const result = classifyTrack(
    track(
      stationary(80, { behavior: "Chase", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
        movement_hold: i >= 8 && i < 68 ? "DoorWait" : null,
      })),
    ),
    { pass: "idle" },
  );
  assert.notEqual(result.verdict, "wedged");
});

test("a hold that ends well before the window closes still leaves a wedge", () => {
  const result = classifyTrack(
    track(
      stationary(30, { behavior: "Chase", outcome: "Full" }).map((s, i) => ({
        ...s,
        live_stall_seconds: i % 6,
        movement_hold: i < 4 ? "Pivot" : null,
      })),
    ),
    { pass: "idle" },
  );
  assert.equal(result.verdict, "wedged");
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
