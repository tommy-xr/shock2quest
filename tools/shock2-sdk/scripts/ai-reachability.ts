/**
 * AI reachability harness: does every AI in a mission actually get where it
 * is going?
 *
 * Two passes per mission, each on a freshly launched runtime:
 *   idle  - no player input, the AIs run their authored patrols
 *   chase - the player stands at a fixed spot, DebugForceChase pins every AI
 *           onto them, and we watch who arrives
 *
 * Every AI is sampled every N frames (position, behavior, alertness, its
 * live path + stall) and classified at the end of the pass (see
 * src/ai-reachability.ts). Writes <out>/<mission>.json with every sample, a
 * <out>/summary.md table, and per-pass trail maps drawn over the mission's
 * nav cells.
 *
 *   npm run ai-reachability -- --mission medsci2.mis --out /tmp/aire
 *   npm run ai-reachability -- --all --experimental nav_bridges
 *   npm run ai-reachability -- --all --pass chase --experimental nav_bridges
 */

import { spawnSync } from "node:child_process";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

import { findRepoRoot, GameServer, HttpError } from "../src/index.js";
import type { Game } from "../src/index.js";
import {
  classifyTrack,
  distance3,
  tally,
  VERDICTS,
  type AiSample,
  type AiTrack,
  type Classification,
} from "../src/ai-reachability.js";
import type { EntityDetailResult, Vec3 } from "../src/types.js";

/**
 * Where the player stands for the chase pass, per mission. An empty list
 * means "wherever the mission spawns the player"; add positions here to
 * probe more of a deck.
 */
const CHASE_POSITIONS: Record<string, Vec3[]> = {
  // The medsci1 spawn is the sealed cryo recovery room (AI-unreachable by
  // design), so chase from the open deck instead.
  "medsci1.mis": [[-14.0, 0.5, -30.0]],
};

type PassSelector = "idle" | "chase" | "both";

interface Args {
  missions: string[];
  out: string;
  idleFrames: number;
  chaseFrames: number;
  sampleEvery: number;
  experimental: string[];
  pass: PassSelector;
  /** Re-classify the stored JSON in this directory instead of running the game. */
  reclassify?: string;
}

function parseArgs(argv: string[]): Args {
  const args: Args = {
    missions: [],
    out: path.join("/tmp", "ai-reachability"),
    idleFrames: 3600,
    chaseFrames: 1800,
    sampleEvery: 30,
    experimental: [],
    pass: "both",
  };
  let all = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${a} needs a value`);
      return v;
    };
    switch (a) {
      case "--mission":
        args.missions.push(value());
        break;
      case "--all":
        all = true;
        break;
      case "--out":
        args.out = value();
        break;
      case "--idle-frames":
        args.idleFrames = positiveInt(a, value());
        break;
      case "--chase-frames":
        args.chaseFrames = positiveInt(a, value());
        break;
      case "--sample-every":
        args.sampleEvery = positiveInt(a, value());
        break;
      case "--experimental":
        args.experimental.push(...value().split(","));
        break;
      case "--reclassify":
        args.reclassify = value();
        break;
      case "--pass": {
        const v = value();
        if (v !== "idle" && v !== "chase" && v !== "both") {
          throw new Error(`--pass must be idle, chase, or both, got ${v}`);
        }
        args.pass = v;
        break;
      }
      default:
        throw new Error(`unknown argument ${a}`);
    }
  }
  if (all) args.missions = missionsInData();
  // Re-classification reads stored runs; it needs no mission list.
  if (args.reclassify) return args;
  if (args.missions.length === 0) throw new Error("pass --mission <name> or --all");
  // A pass shorter than one sample would report every AI as "no samples".
  for (const [name, frames] of [
    ["--idle-frames", args.idleFrames],
    ["--chase-frames", args.chaseFrames],
  ] as const) {
    if (frames < args.sampleEvery) {
      throw new Error(`${name} (${frames}) must be at least --sample-every (${args.sampleEvery})`);
    }
  }
  return args;
}

function positiveInt(flag: string, raw: string): number {
  const value = Number(raw);
  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`${flag} needs a positive whole number, got ${raw}`);
  }
  return value;
}

function repoRoot(): string {
  const root = findRepoRoot(process.cwd());
  if (!root) throw new Error("could not find the cargo workspace root");
  return root;
}

/**
 * Missions in the game data, from the engine's own discovery - a 25th
 * Anniversary install keeps every .mis inside an archive, so listing the data
 * directory finds nothing there.
 */
function missionsInData(): string[] {
  const result = spawnSync("cargo", ["run", "--release", "-q", "-p", "bench", "--", "path", "missions"], {
    cwd: repoRoot(),
    encoding: "utf8",
  });
  const missions = (result.stdout ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.endsWith(".mis"));
  if (result.status !== 0 || missions.length === 0) {
    throw new Error(`could not list missions: ${result.stderr?.slice(-400) ?? "no output"}`);
  }
  return missions.sort();
}

/**
 * Every AI in the scene, found by the AI runtime properties rather than by
 * name - runtime entity ids are not stable across runs, and creature names
 * vary per mission.
 */
async function discoverAis(game: Game): Promise<EntityDetailResult[]> {
  const { entities } = await game.entities.list({ limit: 5000 });
  const ais: EntityDetailResult[] = [];
  for (const entity of entities) {
    const detail = await game.entities.detail(entity.id).catch((error: unknown) => {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    });
    if (!detail) continue;
    const props = new Set(detail.properties.map((p) => p.name));
    if (props.has("AIAlertness") || props.has("AIBehavior")) ais.push(detail);
  }
  return ais;
}

function prop(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((p) => p.name === name)?.value;
}

/** Yaw (radians) from the entity's [w,x,y,z] rotation quaternion. */
function yawOf(rotation: [number, number, number, number]): number {
  const [w, x, y, z] = rotation;
  return Math.atan2(2 * (w * y + x * z), 1 - 2 * (y * y + z * z));
}

interface PassResult {
  pass: string;
  player: Vec3;
  frames: number;
  tracks: (AiTrack & { classification: Classification })[];
}

async function runPass(
  game: Game,
  passName: "idle" | "chase",
  label: string,
  frames: number,
  sampleEvery: number,
): Promise<PassResult> {
  const playerPos = await game.player.position();
  const player: Vec3 = [playerPos.x, playerPos.y, playerPos.z];

  const ais = await discoverAis(game);
  const tracks = new Map<number, AiTrack>();
  for (const ai of ais) {
    const route = await game.pathfinding.route(ai.position, player);
    // An off-mesh endpoint fails the query exactly like a disconnected one,
    // so only a route between two resolved cells answers "unreachable".
    const resolved = route !== null && route.from_cell !== null && route.to_cell !== null;
    tracks.set(ai.entity_id, {
      entity_id: ai.entity_id,
      name: ai.name,
      template_id: ai.template_id,
      reachable: resolved ? route.reachable : null,
      samples: [],
    });
  }

  const steps = Math.floor(frames / sampleEvery);
  for (let step = 1; step <= steps; step++) {
    await game.step({ frames: sampleEvery });
    const t = (step * sampleEvery) / 60;
    // Re-read the player: a chase pass sends every creature into melee, and
    // distances to a stale position would be fiction if the player is moved.
    const live = await game.player.position();
    const playerNow: Vec3 = [live.x, live.y, live.z];
    const paths = new Map((await game.pathfinding.aiPaths()).map((p) => [p.entity_id, p]));
    for (const [id, track] of tracks) {
      // A dead or despawned AI stops reporting (404); the samples so far
      // still classify. Any other failure is a broken run, not a datum.
      const detail = await game.entities.detail(id).catch((error: unknown) => {
        if (error instanceof HttpError && error.status === 404) return null;
        throw error;
      });
      if (!detail) continue;
      const route = paths.get(track.entity_id);
      const sample: AiSample = {
        t,
        position: detail.position,
        yaw: yawOf(detail.rotation),
        behavior: prop(detail, "AIBehavior"),
        alertness: prop(detail, "AIAlertness"),
        outcome: route?.outcome,
        live_next_waypoint: route?.live_next_waypoint ?? null,
        live_path_len: route?.live_path_len ?? null,
        live_target: route?.live_target ?? null,
        live_stall_seconds: route?.live_stall_seconds ?? null,
        movement_hold: route?.movement_hold ?? null,
        distance: distance3(detail.position, playerNow),
      };
      track.samples.push(sample);
    }
  }

  return {
    pass: label,
    player,
    frames,
    tracks: [...tracks.values()].map((track) => ({
      ...track,
      classification: classifyTrack(track, { pass: passName }),
    })),
  };
}

/** `cargo bn path dump` - the mission's nav cells, for the trail map. */
async function dumpCells(mission: string, outFile: string): Promise<boolean> {
  const result = spawnSync(
    "cargo",
    ["run", "--release", "-q", "-p", "bench", "--", "path", "dump", mission],
    { cwd: repoRoot(), encoding: "utf8", maxBuffer: 512 * 1024 * 1024 },
  );
  if (result.status !== 0 || !result.stdout) {
    console.warn(`  nav-cell dump failed for ${mission}: ${result.stderr?.slice(-400) ?? ""}`);
    return false;
  }
  await writeFile(outFile, result.stdout);
  return true;
}

function renderMap(cellsFile: string, passFile: string, pngFile: string): void {
  const script = path.join(repoRoot(), "tools/shock2-sdk/scripts/render-trail-map.py");
  const result = spawnSync("python3", [script, cellsFile, passFile, pngFile], {
    encoding: "utf8",
  });
  if (result.status !== 0) {
    console.warn(`  trail map failed: ${result.stderr?.slice(-400) ?? ""}`);
  }
}

function passTable(mission: string, pass: PassResult): string {
  const counts = tally(pass.tracks.map((t) => t.classification));
  const cells = VERDICTS.map((v) => `${counts[v]}`).join(" | ");
  return `| ${mission} | ${pass.pass} | ${pass.tracks.length} | ${cells} |`;
}

function wedgeLines(pass: PassResult): string[] {
  return pass.tracks
    .filter((t) => t.classification.verdict === "wedged")
    .map(
      (t) =>
        `  - ${t.name} (template ${t.template_id}) wedged ${t.classification.wedge_seconds?.toFixed(1)}s at (${t.classification
          .wedge_at!.map((c) => c.toFixed(2))
          .join(", ")}) [${pass.pass}]`,
    );
}

/** A stored pass, as written by a previous run. */
interface StoredReport {
  mission: string;
  passes: {
    pass: string;
    tracks: (AiTrack & { classification: Classification })[];
  }[];
}

function countsRow(label: string, counts: Record<string, number>): string {
  return `| ${label} | ${VERDICTS.map((v) => counts[v] ?? 0).join(" | ")} |`;
}

/**
 * Re-run the classifier over a stored run's JSON - no game, no runtime. The
 * stored samples carry whatever fields the runtime published when they were
 * taken, so a re-classification of an older run simply sees no movement_hold.
 */
async function reclassify(dir: string): Promise<void> {
  const files = (await readdir(dir))
    .filter((f) => f.endsWith(".mis.json"))
    .sort();
  const header = `| mission/pass | ${VERDICTS.join(" | ")} |`;
  const divider = `|${"---|".repeat(VERDICTS.length + 1)}`;
  const rows: string[] = [];
  const oldTotals: Record<string, number> = {};
  const newTotals: Record<string, number> = {};

  for (const file of files) {
    const report = JSON.parse(await readFile(path.join(dir, file), "utf8")) as StoredReport;
    for (const pass of report.passes) {
      const passName = pass.pass.startsWith("chase") ? "chase" : "idle";
      const before = tally(pass.tracks.map((t) => t.classification));
      const after = tally(
        pass.tracks.map((t) => classifyTrack(t, { pass: passName as "idle" | "chase" })),
      );
      for (const v of VERDICTS) {
        oldTotals[v] = (oldTotals[v] ?? 0) + (before[v] ?? 0);
        newTotals[v] = (newTotals[v] ?? 0) + (after[v] ?? 0);
      }
      const cells = VERDICTS.map((v) => {
        const a = before[v] ?? 0;
        const b = after[v] ?? 0;
        return a === b ? `${a}` : `${a}→${b}`;
      }).join(" | ");
      rows.push(`| ${report.mission} ${pass.pass} | ${cells} |`);
    }
  }

  console.log(
    [
      `# Re-classified ${dir}`,
      "",
      header,
      divider,
      ...rows,
      countsRow("**stored total**", oldTotals),
      countsRow("**re-classified total**", newTotals),
      "",
    ].join("\n"),
  );
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));
  if (args.reclassify) {
    await reclassify(args.reclassify);
    return;
  }
  await mkdir(args.out, { recursive: true });

  const header = `| mission | pass | AIs | ${VERDICTS.join(" | ")} |`;
  const divider = `|${"---|".repeat(VERDICTS.length + 3)}`;
  const rows: string[] = [];
  const wedges: string[] = [];

  for (const mission of args.missions) {
    console.log(`\n=== ${mission} ===`);
    const passes: PassResult[] = [];

    // Idle pass: nobody touches the controls; the AIs run their patrols.
    if (args.pass === "idle" || args.pass === "both") {
      await using game = await GameServer.launch({
        mission,
        experimental: args.experimental,
      });
      await game.step({ frames: 10 });
      const pass = await runPass(game, "idle", "idle", args.idleFrames, args.sampleEvery);
      passes.push(pass);
      console.log(passTable(mission, pass));
    }

    // Chase passes: one per authored player position (or the spawn).
    if (args.pass === "chase" || args.pass === "both") {
      const positions = CHASE_POSITIONS[mission] ?? [];
      const chaseSpots: (Vec3 | null)[] = positions.length > 0 ? positions : [null];
      for (const [index, spot] of chaseSpots.entries()) {
        await using game = await GameServer.launch({
          mission,
          experimental: args.experimental,
        });
        await game.step({ frames: 10 });
        if (spot) {
          await game.player.teleport({ x: spot[0], y: spot[1], z: spot[2] });
          await game.step({ frames: 10 });
        }
        await game.input.trigger("DebugForceChase");
        await game.step({ frames: 30 });
        const label = chaseSpots.length > 1 ? `chase-${index}` : "chase";
        const pass = await runPass(game, "chase", label, args.chaseFrames, args.sampleEvery);
        passes.push(pass);
        console.log(passTable(mission, pass));
      }
    }

    const report = { mission, experimental: args.experimental, passes };
    const jsonFile = path.join(args.out, `${mission}.json`);
    await writeFile(jsonFile, JSON.stringify(report, null, 1));

    const cellsFile = path.join(args.out, `${mission}-cells.json`);
    const haveCells = await dumpCells(mission, cellsFile);
    for (const pass of passes) {
      rows.push(passTable(mission, pass));
      wedges.push(...wedgeLines(pass));
      if (haveCells) {
        const passFile = path.join(args.out, `${mission}-${pass.pass}-pass.json`);
        await writeFile(passFile, JSON.stringify(pass));
        renderMap(cellsFile, passFile, path.join(args.out, `${mission}-${pass.pass}.png`));
      }
    }
  }

  const summary = [
    "# AI reachability",
    "",
    header,
    divider,
    ...rows,
    "",
    "## Wedges",
    "",
    ...(wedges.length > 0 ? wedges : ["  (none)"]),
    "",
  ].join("\n");
  await writeFile(path.join(args.out, "summary.md"), summary);
  console.log(`\n${summary}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
