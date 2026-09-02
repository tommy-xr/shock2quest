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
  /** Distance to the player at this sample. */
  distance: number;
}

export interface AiTrack {
  entity_id: number;
  name: string;
  template_id: number;
  /**
   * Does the static walk graph connect this AI's start cell to the player's?
   * Null when the scene has no pathfinding data.
   */
  reachable: boolean | null;
  samples: AiSample[];
}

export type Verdict =
  | "arrived"
  | "progressing"
  | "wedged"
  | "no_route"
  | "expected_unreachable"
  | "idle_by_design";

export interface Classification {
  verdict: Verdict;
  /** One line saying which rule fired, with the numbers behind it. */
  reason: string;
  /** Where the AI was stuck (wedged only) - a reproducible seed position. */
  wedge_at?: Vec3;
  /** How long it was stuck there, seconds (wedged only). */
  wedge_seconds?: number;
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

/**
 * The longest span in which the AI stayed put while it had somewhere to be:
 * a stationary window carrying stall time or an active route. Returns the
 * first such window that clears the duration threshold.
 */
function findWedge(
  samples: AiSample[],
  windowSeconds: number,
  displacement: number,
): { at: Vec3; seconds: number } | undefined {
  for (let i = 0; i < samples.length; i++) {
    let j = i;
    while (j + 1 < samples.length && distance3(samples[i].position, samples[j + 1].position) < displacement) {
      j++;
    }
    const seconds = samples[j].t - samples[i].t;
    if (seconds < windowSeconds) continue;

    const window = samples.slice(i, j + 1);
    const behaviors = window.map((s) => s.behavior).filter((b): b is string => Boolean(b));
    // No behavior data at all is not evidence of standing by design - the
    // runtime sometimes omits AIBehavior - so fall back to the path evidence.
    const traveling = behaviors.length === 0 || behaviors.some((b) => LOCOMOTING.has(b));
    const blocked = window.some(
      (s) =>
        (s.live_stall_seconds ?? 0) > 0 ||
        (s.outcome === "Full" && (s.live_path_len ?? 0) > 0),
    );
    if (traveling && blocked) {
      return { at: samples[i].position, seconds };
    }
    // Windows are maximal, so the next one can only start after this one.
    i = j;
  }
  return undefined;
}

/**
 * Classify one AI's pass.
 *
 * Order matters: a wedge is reported even for an AI that could never have
 * reached the player anyway (being physically stuck is a defect on its own),
 * and "expected unreachable" outranks the routing verdicts so a sealed-off
 * or alert-capped AI is not counted as a pathfinding failure.
 */
export function classifyTrack(track: AiTrack, options: ClassifyOptions): Classification {
  const arriveRadius = options.arriveRadius ?? 3.2;
  const windowSeconds = options.wedgeWindowSeconds ?? 6.0;
  const wedgeDisplacement = options.wedgeDisplacement ?? 1.0;
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

  if (options.pass === "chase" && closest <= arriveRadius) {
    return {
      verdict: "arrived",
      reason: `closed to ${closest.toFixed(1)} (<= ${arriveRadius})`,
      ...base,
    };
  }

  const wedge = findWedge(samples, windowSeconds, wedgeDisplacement);
  if (wedge) {
    return {
      verdict: "wedged",
      reason: `stationary ${wedge.seconds.toFixed(1)}s at (${wedge.at.map((c) => c.toFixed(2)).join(", ")}) with a route or stall active`,
      wedge_at: wedge.at,
      wedge_seconds: wedge.seconds,
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
        reason: "alertness stayed Lowest through a forced chase (alert-capped)",
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

  const traveled = distance3(samples[0].position, samples[samples.length - 1].position);
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
  "no_route",
  "expected_unreachable",
  "idle_by_design",
];

export function tally(classifications: Classification[]): Record<Verdict, number> {
  const counts = Object.fromEntries(VERDICTS.map((v) => [v, 0])) as Record<Verdict, number>;
  for (const c of classifications) counts[c.verdict] += 1;
  return counts;
}
