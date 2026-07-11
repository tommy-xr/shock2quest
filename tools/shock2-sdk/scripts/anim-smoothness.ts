/**
 * Capture an animation-smoothness baseline for the pipe hybrid (OG-Pipe) in
 * medsci1: sample GET /v1/entities/:id/animation once per stepped frame for
 * an IDLE phase and a WALKING phase (alertness forced High so the AI
 * locomotes), run the smoothness metrics over both, write a JSON report, and
 * print a summary table.
 *
 *   npm run anim-smoothness [-- /path/to/report.json]
 *
 * Default report path: <repo>/projects/animation-smoothness-baseline.json.
 */

import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

import { findRepoRoot, GameServer } from "../src/index.js";
import type { Game } from "../src/index.js";
import { analyzePhase, type PhaseReport } from "../src/anim-metrics.js";
import type { AnimationState, EntitySummary, Vec3 } from "../src/types.js";

const PORT = 8095;
const MISSION = "medsci1.mis";
const CAPTURE_FRAMES = 600; // 10 s at the fixed 60 Hz step rate
const FPS = 60;

function distance(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}

/**
 * Find the pipe hybrids by NAME - runtime entity ids are NOT stable across
 * launches, so identity must come from the name (or template_id). Returns
 * the OG-Pipes that actually have an animation player, nearest first.
 */
async function findPipeHybrids(game: Game): Promise<EntitySummary[]> {
  const { entities } = await game.entities.list({ filter: "OG-Pipe", limit: 20 });
  const animated: EntitySummary[] = [];
  for (const entity of entities) {
    if (entity.name !== "OG-Pipe") continue;
    const anim = await game.entities.animation(entity.id);
    if (anim && anim.joints.length > 0) animated.push(entity);
  }
  if (animated.length === 0) {
    throw new Error(
      `No animated OG-Pipe found in ${MISSION} (candidates: ${JSON.stringify(
        entities.map((e) => ({ id: e.id, name: e.name })),
      )})`,
    );
  }
  return animated;
}

/** Step one frame at a time and record the animation state after each step. */
async function captureSamples(
  game: Game,
  entityId: number,
  frames: number,
  label: string,
): Promise<AnimationState[]> {
  const samples: AnimationState[] = [];
  for (let i = 0; i < frames; i++) {
    await game.step({ frames: 1 });
    const sample = await game.entities.animation(entityId);
    if (!sample) {
      throw new Error(
        `${label}: animation state became null at sample ${i} (entity ${entityId} despawned?)`,
      );
    }
    samples.push(sample);
    if ((i + 1) % 120 === 0) {
      console.error(`  ${label}: ${i + 1}/${frames} frames (clip: ${sample.clip})`);
    }
  }
  return samples;
}

/**
 * Keep the player out of melee range during the walking capture so the
 * hybrid keeps locomoting instead of switching to attack clips. moveTo is a
 * collision-valid hop, so this never puts the player inside geometry.
 */
async function keepPlayerAway(game: Game, creaturePos: Vec3): Promise<void> {
  const player = await game.player.position();
  const playerPos: Vec3 = [player.x, player.y, player.z];
  const dist = distance(playerPos, creaturePos);
  if (dist >= 12) return;
  const dx = playerPos[0] - creaturePos[0];
  const dz = playerPos[2] - creaturePos[2];
  const len = Math.hypot(dx, dz) || 1;
  await game.player.moveTo({
    x: player.x + (dx / len) * 4.5,
    y: player.y,
    z: player.z + (dz / len) * 4.5,
  });
}

interface WalkingCapture {
  samples: AnimationState[];
  /** Entity moved meaningfully during the capture. */
  locomotionConfirmed: boolean;
  /** A locomotion clip (ogsrun / walk) was observed. */
  sawLocomotionClip: boolean;
  netDisplacement: number;
  totalPathLength: number;
}

/**
 * Force alertness on each candidate in turn and pick the first that actually
 * locomotes (some OG-Pipes get stuck on geometry and run in place - still
 * valid for pose metrics, but locomotion metrics need real movement).
 */
async function selectWalker(
  game: Game,
  candidates: EntitySummary[],
): Promise<EntitySummary> {
  for (const candidate of candidates) {
    await game.entities.sendMessage(candidate.id, {
      type: "SetAlertness",
      level: "High",
    });
    const before = (await game.entities.animation(candidate.id))!.position;
    let moved = 0;
    for (let i = 0; i < 10 && moved <= 2; i++) {
      await game.step({ frames: 30 });
      const state = (await game.entities.animation(candidate.id))!;
      moved = distance(state.position, before);
      await keepPlayerAway(game, state.position);
    }
    console.error(
      `  candidate ${candidate.id} (distance ${candidate.distance.toFixed(1)}): moved ${moved.toFixed(2)} units during warmup`,
    );
    if (moved > 2) return candidate;
  }
  console.error(
    "WARNING: no OG-Pipe locomoted during warmup - using the nearest one (pose metrics still valid, locomotion metrics are not).",
  );
  return candidates[0];
}

async function captureWalking(
  game: Game,
  entityId: number,
): Promise<WalkingCapture> {
  let sawLocomotionClip = false;
  const samples: AnimationState[] = [];
  for (let i = 0; i < CAPTURE_FRAMES; i++) {
    await game.step({ frames: 1 });
    const sample = await game.entities.animation(entityId);
    if (!sample) {
      throw new Error(`walking: animation state became null at sample ${i}`);
    }
    samples.push(sample);
    if (sample.clip && /run|walk/i.test(sample.clip)) sawLocomotionClip = true;
    if (i % 60 === 59) {
      // Re-assert alertness (it decays) and keep out of melee range.
      await game.entities.sendMessage(entityId, {
        type: "SetAlertness",
        level: "High",
      });
      await keepPlayerAway(game, sample.position);
    }
    if ((i + 1) % 120 === 0) {
      console.error(`  walking: ${i + 1}/${CAPTURE_FRAMES} frames (clip: ${sample.clip})`);
    }
  }

  let totalPathLength = 0;
  for (let i = 1; i < samples.length; i++) {
    totalPathLength += distance(samples[i].position, samples[i - 1].position);
  }
  const netDisplacement = distance(
    samples[samples.length - 1].position,
    samples[0].position,
  );

  return {
    samples,
    locomotionConfirmed: netDisplacement > 2 || totalPathLength > 5,
    sawLocomotionClip,
    netDisplacement,
    totalPathLength,
  };
}

function fmt(value: number): string {
  return value.toFixed(4);
}

function printPhaseSummary(name: string, report: PhaseReport): void {
  console.log(`\n=== ${name} (${report.sampleCount} samples, ${report.durationSeconds.toFixed(1)}s) ===`);
  console.log(
    `  clips: ${Object.entries(report.clipFrames)
      .map(([clip, frames]) => `${clip}=${frames}f`)
      .join(", ")}`,
  );
  console.log(`  clip changes: ${report.clipChangeCount}, loop resets: ${report.loopResetCount}`);
  console.log(`  spike threshold (root-relative max-joint dp): ${fmt(report.spikeThreshold)}`);
  console.log(
    `  spikes: ${report.spikes.length} (${report.spikesPerSecond.toFixed(2)}/s)` +
      ` - clip-switch=${report.spikeKinds["clip-switch"]},` +
      ` loop-seam=${report.spikeKinds["loop-seam"]},` +
      ` mid-clip=${report.spikeKinds["mid-clip"]},` +
      ` max magnitude=${fmt(report.maxSpikeMagnitude)}`,
  );
  console.log("  per-frame max-joint |dp|      p50      p95      max     mean");
  console.log(
    `    root-relative          ${fmt(report.localMaxDelta.p50)}  ${fmt(report.localMaxDelta.p95)}  ${fmt(report.localMaxDelta.max)}  ${fmt(report.localMaxDelta.mean)}`,
  );
  console.log(
    `    world                  ${fmt(report.worldMaxDelta.p50)}  ${fmt(report.worldMaxDelta.p95)}  ${fmt(report.worldMaxDelta.max)}  ${fmt(report.worldMaxDelta.mean)}`,
  );
  console.log(
    `  mean max-joint jerk: local=${fmt(report.localMeanMaxJerk)} world=${fmt(report.worldMeanMaxJerk)}`,
  );
  console.log(
    `  entity |dp| p50/p95/max: ${fmt(report.entityDelta.p50)}/${fmt(report.entityDelta.p95)}/${fmt(report.entityDelta.max)}` +
      `  entity accel p95/max: ${fmt(report.entityAccel.p95)}/${fmt(report.entityAccel.max)}`,
  );
  const top = [...report.spikes]
    .sort((a, b) => b.magnitude - a.magnitude)
    .slice(0, 8);
  if (top.length > 0) {
    console.log("  top spikes (frame, kind, magnitude, clip transition):");
    for (const spike of top) {
      console.log(
        `    #${spike.index}  ${spike.kind.padEnd(11)} ${fmt(spike.magnitude)}  ${spike.prevClip ?? "(none)"} -> ${spike.clip ?? "(none)"}@${spike.clipFrame}${spike.blending ? " [blending]" : ""}`,
      );
    }
  }
}

/** Round all numbers deeply so the JSON report stays readable and compact. */
function roundDeep(value: unknown): unknown {
  if (typeof value === "number") {
    return Number.isInteger(value) ? value : Number(value.toFixed(5));
  }
  if (Array.isArray(value)) return value.map(roundDeep);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([k, v]) => [
        k,
        roundDeep(v),
      ]),
    );
  }
  return value;
}

async function main(): Promise<void> {
  const repoRoot = findRepoRoot(import.meta.dirname);
  if (!repoRoot) throw new Error("Could not locate the cargo workspace root");
  const outPath =
    process.argv[2] ??
    path.join(repoRoot, "projects", "animation-smoothness-baseline.json");

  console.error(`Launching debug runtime (${MISSION}, port ${PORT})...`);
  await using game = await GameServer.launch({ mission: MISSION, port: PORT });

  // Let the mission settle before measuring.
  await game.step({ frames: 60 });

  const candidates = await findPipeHybrids(game);
  const target = candidates[0];
  console.error(
    `Idle target: ${target.name} (runtime id ${target.id}, template ${target.template_id}, distance ${target.distance.toFixed(1)}; ${candidates.length} animated OG-Pipes total)`,
  );

  console.error("Capturing IDLE phase...");
  const idleSamples = await captureSamples(game, target.id, CAPTURE_FRAMES, "idle");
  const idle = analyzePhase(idleSamples, { fps: FPS });

  // The "idle" label is only honest if the AI stayed idle - it could have
  // spotted the player and gone into locomotion/attack clips mid-capture.
  const nonIdleClips = [
    ...new Set(
      idleSamples
        .map((s) => s.clip)
        .filter((clip): clip is string => clip !== null && !/idle/i.test(clip)),
    ),
  ];
  const idleConfirmed = nonIdleClips.length === 0;
  if (!idleConfirmed) {
    console.error(
      `WARNING: idle capture contains non-idle clips (${nonIdleClips.join(", ")}) - the AI was not idle; idle metrics are suspect.`,
    );
  }

  console.error("Forcing alertness and selecting a walker...");
  const walker = await selectWalker(game, candidates);
  console.error(`Walking target: runtime id ${walker.id}`);
  const walkingCapture = await captureWalking(game, walker.id);
  const walking = analyzePhase(walkingCapture.samples, { fps: FPS });

  const report = {
    generatedAt: new Date().toISOString(),
    mission: MISSION,
    entity: {
      name: target.name,
      template_id: target.template_id,
      // Runtime ids are not stable across launches; recorded for traceability only.
      idle_runtime_id: target.id,
      walking_runtime_id: walker.id,
      walking_template_id: walker.template_id,
    },
    captureFrames: CAPTURE_FRAMES,
    fps: FPS,
    phases: {
      idle: {
        ...idle,
        idleConfirmed,
        nonIdleClips,
      },
      walking: {
        ...walking,
        locomotion: {
          confirmed: walkingCapture.locomotionConfirmed,
          sawLocomotionClip: walkingCapture.sawLocomotionClip,
          netDisplacement: walkingCapture.netDisplacement,
          totalPathLength: walkingCapture.totalPathLength,
        },
      },
    },
  };

  await mkdir(path.dirname(outPath), { recursive: true });
  await writeFile(outPath, JSON.stringify(roundDeep(report), null, 2));

  printPhaseSummary("IDLE", idle);
  printPhaseSummary("WALKING", walking);
  console.log(
    `\nwalking locomotion: confirmed=${walkingCapture.locomotionConfirmed}` +
      ` sawLocomotionClip=${walkingCapture.sawLocomotionClip}` +
      ` net=${walkingCapture.netDisplacement.toFixed(2)} path=${walkingCapture.totalPathLength.toFixed(2)}`,
  );
  console.log(`\nReport written to ${outPath}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
