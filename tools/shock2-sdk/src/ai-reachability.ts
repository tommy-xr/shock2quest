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
  /** How far it got from the halt afterwards (wedged_then_freed only). */
  freed_travel?: number;
  /**
   * Every halt found in the pass, in order - the raw evidence behind the
   * verdict, kept in the stored JSON so a reviewer can audit a bucket without
   * re-running the pass.
   */
  halts?: HaltEvidence[];
  closest_distance: number;
  final_distance: number;
}

/** One halt, as observed - the audit trail behind a wedge verdict. */
export interface HaltEvidence {
  /** Seconds into the pass at which the unheld stationary stretch started. */
  start_t: number;
  /** Length of that stretch, seconds. */
  seconds: number;
  at: Vec3;
  /** Farthest the AI later got from `at`, world units. */
  escape_distance: number;
  /** Seconds of the halt spent under a published movement hold. */
  held_seconds: number;
  /** Path outcome in effect during the halt, if the AI had ever pathed. */
  outcome?: string;
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
   * How far from the halt the AI must get for it to count as having walked
   * out, world units. Such a span is reported as `wedged_then_freed`, not
   * `wedged`.
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

/**
 * The path outcome in effect at sample `through` - the latest non-null outcome
 * at or before it. Outcomes are sticky (the runtime republishes the last query
 * result), so reading the LAST one in the pass answers "did this AI ever end up
 * without a route", not "did it have a route while it was halted". A wedge is
 * judged against the outcome that was live during the halt.
 */
function outcomeThrough(samples: AiSample[], through: number): string | undefined {
  for (let i = Math.min(through, samples.length - 1); i >= 0; i--) {
    if (samples[i].outcome) return samples[i].outcome;
  }
  return undefined;
}

/** A halt: where it happened, how long, and where the AI stood still until. */
interface StationarySpan {
  /** First sample of the unheld stationary stretch. */
  start: number;
  /**
   * Last sample of the unheld stretch - the halt being judged ends here, so
   * the route it was judged under is the one live at this sample and not one
   * published during a hold that merely followed it.
   */
  end: number;
  /** Last sample of the whole stationary window - escape is measured from here. */
  windowEnd: number;
  at: Vec3;
  seconds: number;
  /** Seconds of the stationary window spent under a movement hold. */
  heldSeconds: number;
}

/** Is this sample reporting a deliberate hold (door wait, pivot)? */
function isHeld(sample: AiSample): boolean {
  return Boolean(sample.movement_hold);
}

/**
 * The longest unbroken stretch of the window in which the AI was NOT under a
 * published movement hold, as [firstSample, seconds]. Time waiting on a door
 * or turning in place is an intentional pause, and a long one used to cross
 * the wedge window on its own.
 *
 * The hold reported at a sample describes the interval that follows it, so a
 * held sample ends the stretch it starts.
 */
function longestUnheldRun(window: AiSample[]): { from: number; to: number; seconds: number } {
  let best = { from: 0, to: 0, seconds: 0 };
  let runStart = 0;
  for (let k = 0; k < window.length; k++) {
    if (isHeld(window[k])) {
      runStart = k + 1;
      continue;
    }
    const seconds = window[k].t - window[runStart].t;
    if (seconds > best.seconds) best = { from: runStart, to: k, seconds };
  }
  return best;
}

/**
 * Every halt in which the AI stayed put while it was actively trying to
 * move: stationary for long enough with no hold to explain it, and with
 * stall time that CHANGES across the window.
 *
 * The change requirement matters - the steering's live record is only
 * refreshed while path-following runs, so an AI that stopped following a path
 * (reached its patrol end, switched to attacking) keeps its last snapshot
 * forever. A frozen snapshot is not evidence of a wedge; a stall clock that
 * ticks (or resets on a backout) is.
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
    if (samples[j].t - samples[i].t < windowSeconds) continue;

    const window = samples.slice(i, j + 1);
    const unheld = longestUnheldRun(window);
    if (unheld.seconds < windowSeconds) continue;

    const behaviors = window.map((s) => s.behavior).filter((b): b is string => Boolean(b));
    // No behavior data at all is not evidence of standing by design - the
    // runtime sometimes omits AIBehavior - so fall back to the path evidence.
    const traveling = behaviors.length === 0 || behaviors.some((b) => LOCOMOTING.has(b));
    const stalls = new Set(window.map((s) => s.live_stall_seconds ?? 0));
    const blocked = stalls.size > 1 && Math.max(...stalls) > 0;
    if (traveling && blocked) {
      let heldSeconds = 0;
      for (let k = i; k < j; k++) {
        if (isHeld(samples[k])) heldSeconds += samples[k + 1].t - samples[k].t;
      }
      spans.push({
        start: i + unheld.from,
        end: i + unheld.to,
        windowEnd: j,
        at: window[unheld.from].position,
        seconds: unheld.seconds,
        heldSeconds,
      });
      // Spans starting inside this one are the same halt seen later.
      i = j;
    }
  }
  return spans;
}

/**
 * Path length walked from `from` onwards - not start-to-end displacement, so
 * a loop patrol that returns to where it started still reads as travel.
 */
export function pathLength(samples: AiSample[], from = 0): number {
  let sum = 0;
  for (let k = from; k + 1 < samples.length; k++) {
    sum += distance3(samples[k].position, samples[k + 1].position);
  }
  return sum;
}

/**
 * How far the AI got from a halt after it ended. Distance from the halt, not
 * path length: an AI oscillating inside its pocket racks up path length
 * without ever leaving.
 */
function escapedBy(samples: AiSample[], span: StationarySpan): number {
  let farthest = 0;
  for (let k = span.windowEnd; k < samples.length; k++) {
    farthest = Math.max(farthest, distance3(span.at, samples[k].position));
  }
  return farthest;
}

/**
 * Classify one AI's pass.
 *
 * Order matters: an AI that was never going anywhere ("expected
 * unreachable") or has no route to the player ("no route") is judged as such
 * BEFORE the wedge check - standing still without a route is not a wedge, and
 * letting `wedged` outrank them made the headline wedge count absorb whole
 * buckets of non-defects. The no-route test reads the outcome that was live
 * during the halt being judged, not the last one of the pass.
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

  // Halts are found up front so that every verdict - not only a wedge - can
  // carry the evidence, and so the no-route check can be asked about the
  // moment being judged rather than about the end of the pass.
  const spans = findWedges(samples, windowSeconds, wedgeDisplacement);
  const escapes = spans.map((span) => escapedBy(samples, span));
  const halts: HaltEvidence[] = spans.map((span, i) => ({
    start_t: samples[span.start].t,
    seconds: span.seconds,
    at: span.at,
    escape_distance: escapes[i],
    held_seconds: span.heldSeconds,
    outcome: outcomeThrough(samples, span.end),
  }));
  const evidence = halts.length > 0 ? { halts } : {};

  const closest = Math.min(...samples.map((s) => s.distance));
  const final = samples[samples.length - 1].distance;
  const base = { closest_distance: closest, final_distance: final, ...evidence };

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

  // The halt this AI is judged on: the first one it does NOT get clear of,
  // else the first halt at all, else the pass as a whole.
  const stuckIndex = escapes.findIndex((escape) => escape <= freedTravelUnits);
  const judgedIndex = stuckIndex >= 0 ? stuckIndex : spans.length > 0 ? 0 : -1;

  // No route means no wedge - but only if the route was missing WHILE the AI
  // stood there. Outcomes are sticky, so an early Partial used to outrank a
  // genuine wedge that happened later with a full route in hand.
  const outcome =
    judgedIndex >= 0
      ? outcomeThrough(samples, spans[judgedIndex].end)
      : outcomeThrough(samples, samples.length - 1);
  if (outcome === "Partial" || outcome === "Failed") {
    const when = judgedIndex >= 0 ? "during the halt" : "at pass end";
    return { verdict: "no_route", reason: `path outcome ${outcome} ${when}`, ...base };
  }

  // A halt the AI walks out of is not a wedge: report the first span it does
  // NOT walk out of, and only if every span was walked out of does the AI
  // land in the separate `wedged_then_freed` bucket (still visible, but not
  // counted against the headline wedge number). "Walked out" means it got
  // clear of the spot, not that it covered distance near it.
  if (stuckIndex >= 0) {
    const stuck = spans[stuckIndex];
    return {
      verdict: "wedged",
      reason: `stationary ${stuck.seconds.toFixed(1)}s at (${stuck.at
        .map((c) => c.toFixed(2))
        .join(", ")}) with a route or stall active`,
      wedge_at: stuck.at,
      wedge_seconds: stuck.seconds,
      ...base,
    };
  }
  if (spans.length > 0) {
    const span = spans[0];
    const freed = escapes[0];
    return {
      verdict: "wedged_then_freed",
      reason: `stationary ${span.seconds.toFixed(1)}s at (${span.at
        .map((c) => c.toFixed(2))
        .join(", ")}), then moved ${freed.toFixed(1)} away`,
      wedge_at: span.at,
      wedge_seconds: span.seconds,
      freed_travel: freed,
      ...base,
    };
  }

  const traveled = pathLength(samples);
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
