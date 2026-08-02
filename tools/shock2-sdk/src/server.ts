import { type ChildProcess, execFile, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join } from "node:path";
import { promisify } from "node:util";

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
   * Reap (SIGKILL) a stale `debug_runtime` already bound to the requested
   * launch port before starting - e.g. an orphan left by a prior agent
   * session that died without reaching `/v1/shutdown` (see #786). Only ever
   * targets the exact port this launch asked for (`port ?? 8080`); a
   * fallback port picked by the free-port walk-up on a bind race is never
   * touched. Default true.
   */
  reapStale?: boolean;
}

/** Walk up from a directory until a cargo workspace root is found. */
export function findRepoRoot(startDir: string): string | undefined {
  let dir = startDir;
  for (;;) {
    const manifest = join(dir, "Cargo.toml");
    if (existsSync(manifest) && readFileSync(manifest, "utf8").includes("[workspace]")) {
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

const execFileAsync = promisify(execFile);

/** A live process with a listening socket on a port, as reported by the OS. */
export interface PortProcess {
  pid: number;
  /** Full command line (not the truncated program name `lsof` reports). */
  command: string;
}

/**
 * Finds processes with a listening TCP socket on `port`, via `lsof` (pid
 * discovery) + `ps` (full command line - `lsof`'s COMMAND column truncates to
 * 9 characters and drops arguments, so it can't be used to confirm this is a
 * `debug_runtime` bound to the port we think it is). Best-effort: returns []
 * if either tool is unavailable or reports nothing, rather than throwing -
 * callers treat that the same as "no stale process found".
 */
export async function findProcessesOnPort(port: number): Promise<PortProcess[]> {
  let stdout: string;
  try {
    ({ stdout } = await execFileAsync("lsof", [
      "-nP",
      `-iTCP:${port}`,
      "-sTCP:LISTEN",
      "-t",
    ]));
  } catch {
    // No listener on the port (lsof exits non-zero), or lsof isn't installed.
    return [];
  }
  const pids = [...new Set(stdout.split("\n").map((line) => line.trim()).filter(Boolean))].map(
    Number,
  );

  const processes: PortProcess[] = [];
  for (const pid of pids) {
    try {
      const { stdout: command } = await execFileAsync("ps", ["-p", String(pid), "-o", "command="]);
      processes.push({ pid, command: command.trim() });
    } catch {
      // Process exited between the lsof snapshot and this lookup - skip it.
    }
  }
  return processes;
}

/**
 * True when `command` is a `debug_runtime` invocation bound to exactly
 * `port` - checked by tokenizing the full command line (not a raw substring
 * match, which a name like `debug_runtime_proxy` or a `--port 8080` vs.
 * `--port 808` collision could fool): the executable's basename must be
 * exactly `debug_runtime`, and its args must contain the literal token pair
 * `--port <port>`.
 */
function isDebugRuntimeOnPort(command: string, port: number): boolean {
  const argv = command.trim().split(/\s+/);
  const exe = argv[0]?.split("/").pop();
  if (exe !== "debug_runtime") return false;
  const portFlagIndex = argv.indexOf("--port");
  return portFlagIndex !== -1 && argv[portFlagIndex + 1] === String(port);
}

/**
 * Kill any `debug_runtime` process bound to `port` (see
 * `isDebugRuntimeOnPort` for the exact match criteria - never kills an
 * unrelated process, or a `debug_runtime` that merely mentions this port
 * without actually being bound to it). Best-effort: swallows discovery/kill
 * errors, since a permission failure or the process exiting mid-reap just
 * means the subsequent bind proceeds normally (or fails loudly on its own).
 *
 * Returns the pids it killed, mainly for tests.
 */
export async function reapStaleRuntimeOnPort(
  port: number,
  deps: {
    findProcesses?: (port: number) => Promise<PortProcess[]>;
    kill?: (pid: number, signal: NodeJS.Signals) => void;
  } = {},
): Promise<number[]> {
  const findProcesses = deps.findProcesses ?? findProcessesOnPort;
  const kill = deps.kill ?? ((pid, signal) => process.kill(pid, signal));

  let candidates: PortProcess[];
  try {
    candidates = await findProcesses(port);
  } catch {
    return [];
  }

  const toKill = candidates.filter((p) => isDebugRuntimeOnPort(p.command, port));

  const killed: number[] = [];
  for (const { pid } of toKill) {
    try {
      kill(pid, "SIGKILL");
      killed.push(pid);
    } catch {
      // Already gone, or no permission - nothing more we can do.
    }
  }
  if (killed.length > 0) {
    // Wait for the OS to actually release the socket before returning, so
    // the caller's very next bind probe (`findFreePort`) sees the port as
    // free instead of racing the kill - a fixed short sleep isn't reliably
    // enough under load (see #786 PR discussion), so poll with a bound.
    const deadline = Date.now() + 2000;
    while (!(await portIsFree(port)) && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
  return killed;
}

/**
 * Indirection for `GameServer.launch`'s reap call, so tests can swap it out
 * without spawning a real runtime. ES module exports are read-only live
 * bindings (reassigning the `reapStaleRuntimeOnPort` export from outside
 * this module throws), so a plain mutable object is the seam instead - only
 * the property write needs to be assignable, not the export itself. Not
 * meant to be used outside tests.
 */
export const reapHooks = {
  reapStaleRuntimeOnPort,
};

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
    const server = new GameServer(new HttpClient(baseUrl), undefined, [], undefined, undefined);
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
    // Only the exact requested port - never a fallback port from the
    // bind-race retry loop below (see reapStale's doc comment). Skip
    // entirely when THIS process already reserved the port for a live
    // sibling GameServer (findFreePort below, `reservedPorts`) - that's not
    // a stale orphan, it's our own in-flight launch, and killing it would
    // destroy a live instance for no benefit (the walk-up below would still
    // skip the port via `reservedPorts` regardless of whether we killed it).
    if ((options.reapStale ?? true) && !reservedPorts.has(hint)) {
      await reapHooks.reapStaleRuntimeOnPort(hint);
    }
    let lastError: unknown;
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        return await GameServer.launchOnce(options, hint + attempt);
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
    portHint: number,
  ): Promise<GameServer> {
    const port = await findFreePort(portHint);
    const repoRoot = options.repoRoot ?? findRepoRoot(process.cwd());
    if (repoRoot === undefined) {
      throw new Error(
        "Could not find cargo workspace root; pass repoRoot explicitly",
      );
    }

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
