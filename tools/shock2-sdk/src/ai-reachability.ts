/**
 * Classification of AI reachability samples: given one AI's trail over a
 * pass, decide whether it reached the player, is still making progress, is
 * physically wedged, failed to route, or was never going anywhere.
 *
 * Pure over the samples so it is unit-testable without a running game (the
 * harness that collects the samples lives in scripts/ai-reachability.ts).
 */

import type { Vec3 } from "./types.js";

/** One observation of an AI, taken every N stepped frames. */
export interface AiSample {
  /** Seconds of simulation since the pass started. */
  t: number;
  position: Vec3;
  /** Facing, radians. */
  yaw: number;
  /** AIBehavior ("Patrol", "Chase", ...); absent when the runtime omits it. */
  behavior?: string;
  /** AIAlertness ("Lowest".."High"); absent when the runtime omits it. */
  alertness?: string;
  /** Latest path outcome: "Full" | "Partial" | "Failed". */
  outcome?: string;
  live_next_waypoint?: number | null;
  live_path_len?: number | null;
  live_target?: Vec3 | null;
  live_stall_seconds?: number | null;
  /**
   * Why the AI is deliberately standing still ("DoorWait" | "Pivot"), null
   * when it is not. Absent on runtimes older than the field.
   */
  movement_hold?: string | null;
  /** Distance to the player at this sample. */
  distance: number;
}

export interface AiTrack {
  entity_id: number;
  name: string;
  template_id: number;
  /**
   * Does the static walk graph connect this AI's start cell to the player's?
   * Null when the answer is unknown - no pathfinding data, or either end was
   * off the nav mesh at pass start (a query from an off-mesh point fails the
   * same way a disconnected one does, and must not be read as "by design").
   */
  reachable: boolean | null;
  samples: AiSample[];
}

export type Verdict =
  | "arrived"
  | "progressing"
  | "wedged"
  | "wedged_then_freed"
  | "no_route"
  | "expected_unreachable"
  | "idle_by_design";

export interface Classification {
  verdict: Verdict;
  /** One line saying which rule fired, with the numbers behind it. */
  reason: string;
  /** Where the AI was stuck - a reproducible seed position. */
  wedge_at?: Vec3;
  /** How long it was stuck there, seconds. */
  wedge_seconds?: number;
  /** Units traveled after the stuck span ended (wedged_then_freed only). */
  freed_travel?: number;
  closest_distance: number;
  final_distance: number;
}

export interface ClassifyOptions {
  pass: "idle" | "chase";
  /**
   * Distance at which a chasing AI counts as having reached the player.
   * Defaults to the engine's own melee reach (MELEE_ATTACK_RANGE).
   */
  arriveRadius?: number;
  /** Minimum stationary span that counts as a wedge, seconds. */
  wedgeWindowSeconds?: number;
  /** Displacement below which the AI counts as not moving, world units. */
  wedgeDisplacement?: number;
  /**
   * Travel after a stationary span that means the AI walked out of it, world
   * units. Such a span is reported as `wedged_then_freed`, not `wedged`.
   */
  freedTravelUnits?: number;
}

/** Behaviors that mean the AI is trying to travel somewhere. */
const LOCOMOTING = new Set(["Patrol", "Wander", "Chase", "Search"]);

/** Behaviors that never travel, so failing to arrive is not a defect. */
const NON_TRAVELING = new Set(["ScriptedSequence", "Noop", "Dead", "SelfDestruct"]);

export function distance3(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/** Latest non-null path outcome in the pass, if the AI ever pathed. */
function lastOutcome(samples: AiSample[]): string | undefined {
  for (let i = samples.length - 1; i >= 0; i--) {
    if (samples[i].outcome) return samples[i].outcome;
  }
  return undefined;
}

/** A stationary span: sample indices, where, and how long it lasted. */
interface StationarySpan {
  start: number;
  end: number;
  at: Vec3;
  seconds: number;
  heldSeconds: number;
}

/** Is this sample reporting a deliberate hold (door wait, pivot)? */
function isHeld(sample: AiSample): boolean {
  return Boolean(sample.movement_hold);
}

/**
 * Every span in which the AI stayed put while it was actively trying to
 * move: stationary for long enough, with stall time that CHANGES across the
 * window.
 *
 * The change requirement matters - the steering's live record is only
 * refreshed while path-following runs, so an AI that stopped following a path
 * (reached its patrol end, switched to attacking) keeps its last snapshot
 * forever. A frozen snapshot is not evidence of a wedge; a stall clock that
 * ticks (or resets on a backout) is.
 *
 * Time the AI spends under a published movement hold (waiting on a door,
 * turning in place) does not count toward the window - those are intentional
 * pauses, and a long one used to cross the window on its own.
 */
function findWedges(
  samples: AiSample[],
  windowSeconds: number,
  displacement: number,
): StationarySpan[] {
  const spans: StationarySpan[] = [];
  for (let i = 0; i < samples.length; i++) {
    let j = i;
    while (j + 1 < samples.length && distance3(samples[i].position, samples[j + 1].position) < displacement) {
      j++;
    }
    const seconds = samples[j].t - samples[i].t;
    // The hold reported at a sample describes the interval that follows it.
    let heldSeconds = 0;
    for (let k = i; k < j; k++) {
      if (isHeld(samples[k])) heldSeconds += samples[k + 1].t - samples[k].t;
    }
    if (seconds - heldSeconds < windowSeconds) continue;

    const window = samples.slice(i, j + 1);
    const behaviors = window.map((s) => s.behavior).filter((b): b is string => Boolean(b));
    // No behavior data at all is not evidence of standing by design - the
    // runtime sometimes omits AIBehavior - so fall back to the path evidence.
    const traveling = behaviors.length === 0 || behaviors.some((b) => LOCOMOTING.has(b));
    const stalls = new Set(window.map((s) => s.live_stall_seconds ?? 0));
    const blocked = stalls.size > 1 && Math.max(...stalls) > 0;
    if (traveling && blocked) {
      spans.push({ start: i, end: j, at: samples[i].position, seconds, heldSeconds });
      // Spans starting inside this one are the same halt seen later.
      i = j;
    }
  }
  return spans;
}

/** Path length walked after the span ended. */
function travelAfter(samples: AiSample[], span: StationarySpan): number {
  let sum = 0;
  for (let k = span.end; k + 1 < samples.length; k++) {
    sum += distance3(samples[k].position, samples[k + 1].position);
  }
  return sum;
}

/**
 * Classify one AI's pass.
 *
 * Order matters: an AI that was never going anywhere ("expected
 * unreachable") or has no route to the player ("no route") is judged as such
 * BEFORE the wedge check - standing still without a route is not a wedge, and
 * letting `wedged` outrank them made the headline wedge count absorb whole
 * buckets of non-defects.
 */
export function classifyTrack(track: AiTrack, options: ClassifyOptions): Classification {
  const arriveRadius = options.arriveRadius ?? 3.2;
  const windowSeconds = options.wedgeWindowSeconds ?? 6.0;
  const wedgeDisplacement = options.wedgeDisplacement ?? 1.0;
  const freedTravelUnits = options.freedTravelUnits ?? 5.0;
  const samples = track.samples;

  if (samples.length === 0) {
    return {
      verdict: "idle_by_design",
      reason: "no samples",
      closest_distance: Infinity,
      final_distance: Infinity,
    };
  }

  const closest = Math.min(...samples.map((s) => s.distance));
  const final = samples[samples.length - 1].distance;
  const base = { closest_distance: closest, final_distance: final };

  // Arrival means the AI CLOSED on the player: one that started inside melee
  // reach and never moved must still be able to come out as wedged.
  const start = samples[0].distance;
  if (options.pass === "chase" && closest <= arriveRadius && start > arriveRadius) {
    return {
      verdict: "arrived",
      reason: `closed from ${start.toFixed(1)} to ${closest.toFixed(1)} (<= ${arriveRadius})`,
      ...base,
    };
  }

  const behaviors = new Set(samples.map((s) => s.behavior).filter((b): b is string => Boolean(b)));
  if (behaviors.size > 0 && [...behaviors].every((b) => NON_TRAVELING.has(b))) {
    return {
      verdict: "expected_unreachable",
      reason: `never travels (${[...behaviors].join("/")})`,
      ...base,
    };
  }

  // An AI that cannot be roused above Lowest by a forced chase is alert-capped
  // in the mission data (P$AI_AlertC) - it is not supposed to come.
  if (options.pass === "chase") {
    const alertness = samples.map((s) => s.alertness).filter((a): a is string => Boolean(a));
    if (alertness.length > 0 && alertness.every((a) => a === "Lowest")) {
      return {
        verdict: "expected_unreachable",
        // Either the mission caps its alertness (P$AI_AlertC) or the pin never
        // reaches it at all (it only goes to creatures, so turrets and cameras
        // land here too) - in both cases it was never coming.
        reason: "alertness never rose above Lowest under a forced chase",
        ...base,
      };
    }
  }

  // Only the chase pass holds an AI to the player's position: in the idle
  // pass it is walking its own patrol, so its connectivity to the player says
  // nothing about whether it is doing its job.
  if (options.pass === "chase" && track.reachable === false) {
    return {
      verdict: "expected_unreachable",
      reason: "no walk route from its start cell to the player",
      ...base,
    };
  }

  const outcome = lastOutcome(samples);
  if (outcome === "Partial" || outcome === "Failed") {
    return { verdict: "no_route", reason: `last path outcome ${outcome}`, ...base };
  }

  // A halt the AI walks out of is not a wedge: report the first span it does
  // NOT walk out of, and only if every span was walked out of does the AI
  // land in the separate `wedged_then_freed` bucket (still visible, but not
  // counted against the headline wedge number).
  const spans = findWedges(samples, windowSeconds, wedgeDisplacement);
  const stuck = spans.find((span) => travelAfter(samples, span) <= freedTravelUnits);
  if (stuck) {
    const held = stuck.heldSeconds > 0 ? `, ${stuck.heldSeconds.toFixed(1)}s of it held` : "";
    return {
      verdict: "wedged",
      reason: `stationary ${stuck.seconds.toFixed(1)}s at (${stuck.at
        .map((c) => c.toFixed(2))
        .join(", ")}) with a route or stall active${held}`,
      wedge_at: stuck.at,
      wedge_seconds: stuck.seconds,
      ...base,
    };
  }
  if (spans.length > 0) {
    const span = spans[0];
    const freed = travelAfter(samples, span);
    return {
      verdict: "wedged_then_freed",
      reason: `stationary ${span.seconds.toFixed(1)}s at (${span.at
        .map((c) => c.toFixed(2))
        .join(", ")}), then traveled ${freed.toFixed(1)}`,
      wedge_at: span.at,
      wedge_seconds: span.seconds,
      freed_travel: freed,
      ...base,
    };
  }

  // Path length, not start-to-end displacement: a loop patrol returns to where
  // it started and would otherwise read as "never moved".
  const traveled = samples
    .slice(1)
    .reduce((sum, s, i) => sum + distance3(samples[i].position, s.position), 0);
  const everPathed = samples.some(
    (s) => Boolean(s.outcome) || (s.live_path_len ?? 0) > 0 || (s.live_stall_seconds ?? 0) > 0,
  );
  if (traveled < wedgeDisplacement && !everPathed) {
    return { verdict: "idle_by_design", reason: "never moved and never pathed", ...base };
  }

  return {
    verdict: "progressing",
    reason: `moved ${traveled.toFixed(1)}, ended ${final.toFixed(1)} from the player`,
    ...base,
  };
}

/** Counts by verdict, in table order. */
export const VERDICTS: Verdict[] = [
  "arrived",
  "progressing",
  "wedged",
  "wedged_then_freed",
  "no_route",
  "expected_unreachable",
  "idle_by_design",
];

export function tally(classifications: Classification[]): Record<Verdict, number> {
  const counts = Object.fromEntries(VERDICTS.map((v) => [v, 0])) as Record<Verdict, number>;
  for (const c of classifications) counts[c.verdict] += 1;
  return counts;
}
