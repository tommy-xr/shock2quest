import { type ChildProcess, spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

import { HttpClient } from "./client.js";
import { Game } from "./game.js";

export interface LaunchOptions {
  /** Mission file or debug scene, e.g. "medsci1.mis" or "debug_minimal". */
  mission: string;
  /**
   * Port for the debug runtime HTTP server (default 8080). Treated as a
   * HINT: if it is already taken (another agent's test run, a lingering
   * runtime), the next free port is used instead - read `game.baseUrl` for
   * the actual address.
   */
  port?: number;
  /** Experimental feature flags, e.g. ["teleport"]. */
  experimental?: string[];
  /** Extra debug flags, e.g. ["--debug-pathfinding"]. */
  debugFlags?: string[];
  /** Repo root containing the cargo workspace (default: walk up from cwd). */
  repoRoot?: string;
  /**
   * Max time to wait for the server to come up, in milliseconds.
   * Default 300_000 - the first launch may compile the runtime from scratch.
   */
  launchTimeoutMs?: number;
  /** RUST_LOG filter for the spawned process (default "debug_runtime=info,shock2vr=info"). */
  rustLog?: string;
  /** Echo runtime output to this process's stderr (default false). */
  echoLogs?: boolean;
  /**
   * Reap a prior SDK-owned runtime on the requested port before launching.
   * Ownership must be proven by this checkout's lease and the runtime's exact
   * instance id. Disabled by default so concurrent callers keep independent
   * runtimes and use the next free port as usual.
   */
  reapPrevious?: boolean;
}

/** Walk up from a directory until a cargo workspace root is found. */
export function findRepoRoot(startDir: string): string | undefined {
  let dir = startDir;
  for (;;) {
    const manifest = join(dir, "Cargo.toml");
    if (
      existsSync(manifest) &&
      readFileSync(manifest, "utf8").includes("[workspace]")
    ) {
      return dir;
    }
    const parent = dirname(dir);
    if (parent === dir) {
      return undefined;
    }
    dir = parent;
  }
}

const MAX_LOG_LINES = 2000;

/**
 * Ports handed out by findFreePort that are still in use by this process.
 * The OS probe alone can't see a sibling launch that probed the same port
 * but whose child hasn't bound yet (cargo may compile for minutes first).
 */
const reservedPorts = new Set<number>();

interface RuntimeLease {
  port: number;
  instanceId: string;
  pid: number;
}

function canonicalRepoRoot(repoRoot: string): string {
  try {
    return realpathSync.native(repoRoot);
  } catch {
    return resolve(repoRoot);
  }
}

function runtimeLeaseDirectory(repoRoot: string, port: number): string {
  const checkout = createHash("sha256")
    .update(canonicalRepoRoot(repoRoot))
    .digest("hex")
    .slice(0, 24);
  return join(tmpdir(), "shock2-sdk-runtime-leases", checkout, String(port));
}

/** @internal Exported for ownership-safety tests. */
export function runtimeLeasePath(
  repoRoot: string,
  port: number,
  instanceId: string,
): string {
  return join(runtimeLeaseDirectory(repoRoot, port), `${instanceId}.json`);
}

function writeRuntimeLease(repoRoot: string, lease: RuntimeLease): string {
  const path = runtimeLeasePath(repoRoot, lease.port, lease.instanceId);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(lease)}\n`, {
    encoding: "utf8",
    flag: "wx",
    mode: 0o600,
  });
  return path;
}

function removeRuntimeLease(path: string): void {
  try {
    unlinkSync(path);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code !== "ENOENT") throw error;
  }
}

function readRuntimeLeases(
  repoRoot: string,
  port: number,
): Array<{
  lease: RuntimeLease;
  path: string;
}> {
  const directory = runtimeLeaseDirectory(repoRoot, port);
  let names: string[];
  try {
    names = readdirSync(directory);
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    if (code === "ENOENT") return [];
    throw error;
  }

  const leases: Array<{ lease: RuntimeLease; path: string }> = [];
  for (const name of names) {
    if (!name.endsWith(".json")) continue;
    const path = join(directory, name);
    try {
      const value = JSON.parse(
        readFileSync(path, "utf8"),
      ) as Partial<RuntimeLease>;
      if (
        value.port === port &&
        typeof value.instanceId === "string" &&
        Number.isSafeInteger(value.pid) &&
        (value.pid ?? 0) > 0
      ) {
        leases.push({ lease: value as RuntimeLease, path });
      }
    } catch {
      // A corrupt lease cannot prove ownership, so leave it untouched.
    }
  }
  return leases;
}

async function runtimeInstanceId(port: number): Promise<string | undefined> {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/v1/health`, {
      signal: AbortSignal.timeout(1_000),
    });
    if (!response.ok) return undefined;
    const health = (await response.json()) as { instance_id?: unknown };
    return typeof health.instance_id === "string"
      ? health.instance_id
      : undefined;
  } catch {
    return undefined;
  }
}

async function waitForPortToBeFree(
  port: number,
  timeoutMs: number,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  do {
    if (await portIsFree(port)) return true;
    await new Promise((resolve) => setTimeout(resolve, 50));
  } while (Date.now() < deadline);
  return false;
}

function killLeasedProcessTree(pid: number): void {
  if (process.platform === "win32") {
    const result = spawnSync("taskkill", ["/pid", String(pid), "/T", "/F"], {
      stdio: "ignore",
    });
    if (result.error !== undefined || result.status !== 0) {
      throw result.error ?? new Error(`taskkill exited with ${result.status}`);
    }
    return;
  }
  // launch() creates a detached process group whose leader is this pid.
  process.kill(-pid, "SIGKILL");
}

export type ReapResult = "none" | "preserved" | "reaped";

/**
 * Reap the prior runtime only when a checkout-local lease and live instance id
 * agree. A port occupied by any other instance is deliberately preserved.
 */
export async function reapPreviousRuntime(
  repoRoot: string,
  port: number,
): Promise<ReapResult> {
  const leases = readRuntimeLeases(repoRoot, port);
  if (leases.length === 0) return "none";

  const liveInstanceId = await runtimeInstanceId(port);
  const owned = leases.find(({ lease }) => lease.instanceId === liveInstanceId);
  if (owned === undefined) {
    if (liveInstanceId === undefined && (await portIsFree(port))) {
      for (const entry of leases) removeRuntimeLease(entry.path);
      return "none";
    }
    return "preserved";
  }

  try {
    await fetch(`http://127.0.0.1:${port}/v1/shutdown`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ instance_id: owned.lease.instanceId }),
      signal: AbortSignal.timeout(2_000),
    });
  } catch {
    // The runtime can close the connection while processing shutdown. Treat
    // this as ambiguous and check the port before deciding whether to escalate.
  }
  if (!(await waitForPortToBeFree(port, 2_000))) {
    // Recheck immediately before terminating the recorded process tree. This
    // prevents a stale lease from targeting a runtime that replaced it.
    if ((await runtimeInstanceId(port)) !== owned.lease.instanceId) {
      return "preserved";
    }
    killLeasedProcessTree(owned.lease.pid);
    if (!(await waitForPortToBeFree(port, 5_000))) {
      throw new Error(
        `owned debug_runtime ${owned.lease.instanceId} on port ${port} did not exit`,
      );
    }
  }

  for (const entry of leases) removeRuntimeLease(entry.path);
  return "reaped";
}

/** Children that must not outlive this process (see hookSignalCleanup). */
const liveServers = new Set<GameServer>();
let signalCleanupHooked = false;

/**
 * Detached children receive no terminal signals, so a Ctrl-C of the test
 * runner would orphan every runtime it spawned - kill their process groups
 * before dying, then re-raise the signal for the default behavior.
 */
function hookSignalCleanup(): void {
  if (signalCleanupHooked) return;
  signalCleanupHooked = true;
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.once(signal, () => {
      for (const server of liveServers) {
        server.killProcessTreeForSignalCleanup();
      }
      process.kill(process.pid, signal);
    });
  }
}

/** Max lines included in a launch-failure error message. */
const MAX_ERROR_LOG_LINES = 120;

/** How many ports to try, walking up from the requested one. */
const MAX_PORT_ATTEMPTS = 50;

/** Resolves true when the port can be bound on localhost right now. */
function portIsFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const probe = createServer();
    probe.once("error", () => resolve(false));
    probe.listen({ port, host: "127.0.0.1" }, () => {
      probe.close(() => resolve(true));
    });
  });
}

/**
 * Walk up from `start` to the first free port. Multiple agents commonly run
 * e2e suites with the same hardcoded test ports on one machine; treating the
 * port as a hint keeps them out of each other's way.
 */
export async function findFreePort(start: number): Promise<number> {
  for (let port = start; port < start + MAX_PORT_ATTEMPTS; port++) {
    if (!reservedPorts.has(port) && (await portIsFree(port))) {
      reservedPorts.add(port);
      return port;
    }
  }
  throw new Error(
    `no free port found in [${start}, ${start + MAX_PORT_ATTEMPTS})`,
  );
}

/**
 * Pick the most useful slice of runtime output for an error message.
 *
 * If the output contains a panic, return everything from the (last) panic
 * line onward - that keeps the message and the full backtrace together.
 * Otherwise fall back to the last few lines.
 */
export function formatCrashOutput(logLines: string[]): string {
  const panicIndex = logLines.findLastIndex((line) =>
    line.includes("panicked at"),
  );
  // Include a couple of lines of context before the panic itself.
  const start = panicIndex >= 0 ? Math.max(0, panicIndex - 2) : -30;
  return logLines.slice(start).slice(0, MAX_ERROR_LOG_LINES).join("\n");
}

/**
 * A Game whose debug runtime process is owned by this SDK.
 *
 * Supports `await using` (async disposal) for automatic shutdown:
 *
 * ```ts
 * await using game = await GameServer.launch({ mission: "medsci1.mis" });
 * ```
 */
export class GameServer extends Game implements AsyncDisposable {
  private constructor(
    client: HttpClient,
    private readonly child: ChildProcess | undefined,
    private readonly logLines: string[],
    private readonly instanceId: string | undefined,
    private readonly port: number | undefined,
    private readonly leasePath: string | undefined,
  ) {
    super(client);
  }

  /** The spawned child has exited (normally or by signal). */
  private childIsDead(): boolean {
    return (
      this.child !== undefined &&
      (this.child.exitCode !== null || this.child.signalCode !== null)
    );
  }

  private releaseResources(): void {
    if (this.port !== undefined) {
      reservedPorts.delete(this.port);
    }
    liveServers.delete(this);
    if (this.leasePath !== undefined) {
      removeRuntimeLease(this.leasePath);
    }
  }

  /** Recent stdout/stderr from the spawned runtime (ring buffer). */
  logs(): string[] {
    return [...this.logLines];
  }

  /** @internal Used by the module-level signal cleanup hook. */
  killProcessTreeForSignalCleanup(): void {
    this.killProcessTree();
  }

  /** Connect to an already-running debug runtime. shutdown() will stop it; dispose will not spawn-kill anything. */
  static async connect(baseUrl = "http://127.0.0.1:8080"): Promise<GameServer> {
    const server = new GameServer(
      new HttpClient(baseUrl),
      undefined,
      [],
      undefined,
      undefined,
      undefined,
    );
    await server.health();
    return server;
  }

  /**
   * Spawn a debug runtime via `cargo run -p debug_runtime` and wait for it
   * to be ready. Retries on the next free port when another process wins a
   * port race (bind failure / foreign instance id) between our probe and
   * the child's bind.
   */
  static async launch(options: LaunchOptions): Promise<GameServer> {
    const hint = options.port ?? 8080;
    const repoRoot = options.repoRoot ?? findRepoRoot(process.cwd());
    if (repoRoot === undefined) {
      throw new Error(
        "Could not find cargo workspace root; pass repoRoot explicitly",
      );
    }
    if (options.reapPrevious) {
      await reapPreviousRuntime(repoRoot, hint);
    }
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        return await GameServer.launchOnce(options, repoRoot, hint + attempt);
      } catch (error) {
        lastError = error;
        const message = String(error);
        // "failed to bind" comes from the runtime's own logs (it binds in
        // main() before the game starts); a crash for any other reason -
        // bad mission, panic during load - must NOT retry.
        const lostPortRace =
          message.includes("different debug_runtime instance") ||
          message.includes("failed to bind");
        if (!lostPortRace) {
          throw error;
        }
      }
    }
    throw lastError;
  }

  private static async launchOnce(
    options: LaunchOptions,
    repoRoot: string,
    portHint: number,
  ): Promise<GameServer> {
    const port = await findFreePort(portHint);

    // Identifies OUR runtime: /v1/health echoes it, and readiness checks
    // reject a different instance on the same port (e.g. another agent's
    // runtime grabbing the port between our free-port probe and the child's
    // bind). Cross-talk with a foreign runtime looks like impossible test
    // failures, so fail loudly instead.
    const instanceId = randomUUID();

    const args = [
      "run",
      "-p",
      "debug_runtime",
      "--",
      "--mission",
      options.mission,
      "--port",
      String(port),
      "--instance-id",
      instanceId,
      ...(options.experimental?.length
        ? ["--experimental", options.experimental.join(",")]
        : []),
      ...(options.debugFlags ?? []),
    ];

    const logLines: string[] = [];
    const child = spawn("cargo", args, {
      cwd: repoRoot,
      env: {
        ...process.env,
        RUST_LOG: options.rustLog ?? "debug_runtime=info,shock2vr=info",
        // Capture a callstack in the logs if the game thread panics
        // (respects an explicit override, e.g. RUST_BACKTRACE=full)
        RUST_BACKTRACE: process.env.RUST_BACKTRACE ?? "1",
      },
      stdio: ["ignore", "pipe", "pipe"],
      // Own process group, so the kill fallback can take out the whole tree.
      // `cargo run` execs the game binary as a child; killing only cargo
      // orphans a runtime that keeps the port bound and burns CPU forever.
      detached: true,
    });

    let leasePath: string;
    try {
      if (child.pid === undefined) {
        throw new Error("spawned debug_runtime has no process id");
      }
      leasePath = writeRuntimeLease(repoRoot, {
        port,
        instanceId,
        pid: child.pid,
      });
    } catch (error) {
      if (child.pid !== undefined) {
        try {
          killLeasedProcessTree(child.pid);
        } catch {
          // Preserve the lease error, which is the actionable launch failure.
        }
      }
      reservedPorts.delete(port);
      throw error;
    }

    const capture = (chunk: Buffer) => {
      for (const line of chunk.toString().split("\n")) {
        if (line.length === 0) continue;
        logLines.push(line);
        if (logLines.length > MAX_LOG_LINES) logLines.shift();
        if (options.echoLogs) process.stderr.write(`[debug_runtime] ${line}\n`);
      }
    };
    child.stdout?.on("data", capture);
    child.stderr?.on("data", capture);

    const server = new GameServer(
      new HttpClient(`http://127.0.0.1:${port}`),
      child,
      logLines,
      instanceId,
      port,
      leasePath,
    );
    liveServers.add(server);
    hookSignalCleanup();

    try {
      await server.waitUntilReady(options.launchTimeoutMs ?? 300_000);
    } catch (error) {
      server.killProcessTree();
      server.releaseResources();
      throw new Error(
        `debug_runtime failed to start: ${error}\nRecent output:\n${formatCrashOutput(logLines)}`,
      );
    }

    return server;
  }

  /**
   * SIGKILL the child's whole process group (cargo + the game binary it
   * exec'd). Falls back to killing just the direct child if the group is
   * already gone.
   */
  private killProcessTree(): void {
    const pid = this.child?.pid;
    if (pid === undefined) {
      return;
    }
    try {
      // Negative pid = the process group created by detached: true
      process.kill(-pid, "SIGKILL");
    } catch {
      // ESRCH: the group is already gone. Deliberately NO fallback to a
      // positive-pid kill - the pid may have been reused by an unrelated
      // process by now.
    }
  }

  private async waitUntilReady(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      if (this.childIsDead()) {
        throw new Error(
          `process exited early (code ${this.child?.exitCode}, signal ${this.child?.signalCode})`,
        );
      }
      try {
        // Verify we're talking to OUR instance before trusting the port. A
        // different (or missing) instance id means another process holds the
        // port - keep waiting only if our child is still alive, since the
        // holder may be an older runtime mid-shutdown.
        const health = await this.client.get<{ instance_id?: string | null }>(
          "/v1/health",
        );
        if (
          this.instanceId !== undefined &&
          health.instance_id !== this.instanceId
        ) {
          throw new Error(
            `port is held by a different debug_runtime instance (expected ${this.instanceId}, got ${health.instance_id ?? "none"}) - likely another agent's test run`,
          );
        }
        // /v1/info round-trips through the game loop's command channel, so
        // it only succeeds once the game thread is actually running. (The
        // HTTP server starts before - and can outlive - the game thread,
        // so /v1/health alone would accept a runtime whose game thread
        // crashed during mission load.)
        await this.info();
        return;
      } catch (error) {
        if (Date.now() >= deadline) {
          throw new Error(`server not ready after ${timeoutMs}ms: ${error}`);
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
  }

  /** Gracefully stop the runtime; escalates to a process-group SIGKILL if it doesn't exit. */
  override async shutdown(): Promise<void> {
    // If our child already exited, the port is no longer ours - another
    // agent's runtime may have rebound it, and an HTTP shutdown would stop
    // THEIR instance. Only speak to the port while our child owns it.
    if (!this.childIsDead()) {
      try {
        // The runtime rejects shutdowns without its instance id, so a stale
        // client on another checkout can't kill it - include ours.
        await this.client.post("/v1/shutdown", {
          instance_id: this.instanceId,
        });
      } catch {
        // Server may already be down; fall through to process cleanup.
      }
    }
    const child = this.child;
    if (child === undefined || this.childIsDead()) {
      this.releaseResources();
      return;
    }
    await new Promise<void>((resolve) => {
      const killTimer = setTimeout(() => {
        this.killProcessTree();
      }, 10_000);
      child.once("exit", () => {
        clearTimeout(killTimer);
        resolve();
      });
    });
    this.releaseResources();
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.shutdown();
  }
}
