import { type ChildProcess, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { HttpClient } from "./client.js";
import { Game } from "./game.js";

export interface LaunchOptions {
  /** Mission file or debug scene, e.g. "medsci1.mis" or "debug_minimal". */
  mission: string;
  /**
   * Exact port for the debug runtime HTTP server. Omit it (the default) and
   * the runtime binds an OS-assigned ephemeral port, which the SDK learns
   * from the runtime's own `SHOCK2QUEST_PORT` line - that has no collision
   * window at all, so concurrent launches can never fight over a port.
   *
   * Pass one only when something OUTSIDE the SDK must know the address up
   * front (`adb forward` for the Quest, a hand-run `curl`). Then it is an
   * exact request: the runtime binds it or exits loudly. Either way,
   * `game.baseUrl` reports the address actually in use.
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
 * `--idle-timeout-secs` for SDK-launched runtimes, well under the runtime's
 * own 30-minute default.
 *
 * The runtime's default has to survive a *hand* launch, where minutes of
 * think-time between two `curl`s is normal. The SDK's situation is much
 * tighter: it owns the process, shuts it down on dispose, and kills the
 * process group on SIGINT/SIGTERM, so the watchdog is only ever reached when
 * the owning process dies without running any of that (SIGKILL, an OOM, a
 * yanked CI runner). Every SDK call is an HTTP request and an in-flight one
 * never counts as idle - a `step({duration: "600s"})` holds the clock open for
 * its whole run - so the only real gap is a caller pausing between calls.
 * Ten minutes clears the longest such pause by a wide margin - the worst case
 * is a runtime held while a sibling launch waits out a cold `cargo` build,
 * which the launch timeout itself caps at five minutes - and still bounds a
 * SIGKILL leak (~700 MB) to ten minutes instead of thirty.
 */
const SDK_IDLE_TIMEOUT_SECS = 600;

/** Children that must not outlive this process (see hookSignalCleanup). */
const liveServers = new Set<GameServer>();
/**
 * Spawned runtimes that do not have a GameServer yet - a launch is between
 * `spawn` and the port marker, which can be MINUTES while cargo compiles.
 * They need the same signal cleanup; without it a Ctrl-C during a cold build
 * orphans the very runtime it was building.
 */
const pendingChildren = new Set<ChildProcess>();
let signalCleanupHooked = false;

/**
 * SIGKILL a detached child's whole process group (cargo + the game binary it
 * exec'd). Killing only cargo orphans a runtime that keeps the port bound and
 * burns CPU forever.
 */
function killProcessGroup(pid: number | undefined): void {
  if (pid === undefined) return;
  try {
    // Negative pid = the process group created by detached: true
    process.kill(-pid, "SIGKILL");
  } catch {
    // ESRCH: the group is already gone. Deliberately NO fallback to a
    // positive-pid kill - the pid may have been reused by an unrelated
    // process by now.
  }
}

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
      for (const child of pendingChildren) {
        killProcessGroup(child.pid);
      }
      process.kill(process.pid, signal);
    });
  }
}

/** Max lines included in a launch-failure error message. */
const MAX_ERROR_LOG_LINES = 120;

/** What the runtime announces once it has bound its HTTP port. */
export interface PortMarker {
  /** The port actually bound (never 0, even when 0 was requested). */
  port: number;
  /** Empty when the launcher passed no --instance-id (a hand launch). */
  instanceId: string;
  /** The runtime process's pid, if it reported one. */
  pid?: number;
  /** e.g. "127.0.0.1:54321", if it reported one. */
  address?: string;
}

const PORT_MARKER_PREFIX = "SHOCK2QUEST_PORT ";

/**
 * Parse the runtime's `SHOCK2QUEST_PORT port=.. pid=.. instance_id=.. address=..`
 * line, or undefined if this is not a well-formed marker.
 *
 * Strict about the two fields the SDK acts on - a half-read port would send
 * the client at the wrong address - and lenient about the rest: `pid` and
 * `address` are reported for humans, so a future change to them must not break
 * every launch. The runtime sanitizes instance ids into the `key=value`
 * framing, so plain splitting is enough.
 */
export function parsePortMarker(line: string): PortMarker | undefined {
  const index = line.indexOf(PORT_MARKER_PREFIX);
  if (index < 0) return undefined;
  const fields = new Map<string, string>();
  for (const field of line.slice(index + PORT_MARKER_PREFIX.length).trim().split(/\s+/)) {
    const eq = field.indexOf("=");
    if (eq > 0) fields.set(field.slice(0, eq), field.slice(eq + 1));
  }
  const port = Number(fields.get("port"));
  const instanceId = fields.get("instance_id");
  if (!Number.isInteger(port) || port <= 0 || port > 65535) return undefined;
  if (instanceId === undefined) return undefined;
  const pid = Number(fields.get("pid"));
  return {
    port,
    instanceId,
    pid: Number.isInteger(pid) && pid > 0 ? pid : undefined,
    address: fields.get("address"),
  };
}

/**
 * Split a byte stream into lines, carrying the tail between chunks.
 *
 * A chunk boundary can fall anywhere - including mid-marker - so splitting
 * each chunk on its own would silently lose the one line the launch blocks
 * on. Call the returned `flush` at end-of-stream to emit a final unterminated
 * line.
 */
export function createLineAssembler(onLine: (line: string) => void): {
  push: (chunk: string) => void;
  flush: () => void;
} {
  let carry = "";
  return {
    push(chunk) {
      const parts = (carry + chunk).split("\n");
      carry = parts.pop() ?? "";
      for (const line of parts) {
        if (line.length > 0) onLine(line);
      }
    },
    flush() {
      if (carry.length > 0) onLine(carry);
      carry = "";
    },
  };
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
    const server = new GameServer(new HttpClient(baseUrl), undefined, [], undefined);
    await server.health();
    return server;
  }

  /**
   * Spawn a debug runtime via `cargo run -p debug_runtime` and wait for it to
   * be ready.
   *
   * By default the child binds an ephemeral port (`--port 0`) and announces it
   * on stdout; the SDK reads that line and connects there. That is why there
   * is no free-port probe and no retry loop any more: probing then letting the
   * child bind left a window in which another process could take the port, and
   * *every* mitigation for that window (a reserved-port set, instance-id
   * checks, three attempts on different ports) existed only to paper over it.
   * Letting the OS pick at bind time closes the window instead of guarding it.
   *
   * An explicit `options.port` keeps no probe either, on purpose: a caller
   * naming a port has something outside the SDK pointed at it, so quietly
   * drifting to another port would break exactly that caller. The runtime
   * binds it or exits with `failed to bind`, and the error says so.
   */
  static async launch(options: LaunchOptions): Promise<GameServer> {
    const repoRoot = options.repoRoot ?? findRepoRoot(process.cwd());
    if (repoRoot === undefined) {
      throw new Error(
        "Could not find cargo workspace root; pass repoRoot explicitly",
      );
    }

    // Identifies OUR runtime: the port marker and /v1/health both echo it, so
    // the SDK can prove it is driving the process it spawned rather than some
    // other agent's runtime. Cross-talk with a foreign runtime looks like
    // impossible test failures, so fail loudly instead.
    const instanceId = randomUUID();

    const args = [
      "run",
      "-p",
      "debug_runtime",
      "--",
      "--mission",
      options.mission,
      // 0 = let the OS assign; the child tells us what it got.
      "--port",
      String(options.port ?? 0),
      "--instance-id",
      instanceId,
      "--idle-timeout-secs",
      String(SDK_IDLE_TIMEOUT_SECS),
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
    pendingChildren.add(child);
    hookSignalCleanup();

    let marker: PortMarker | undefined;
    let onMarker: ((found: PortMarker) => void) | undefined;
    const noteLine = (line: string) => {
      logLines.push(line);
      if (logLines.length > MAX_LOG_LINES) logLines.shift();
      if (options.echoLogs) process.stderr.write(`[debug_runtime] ${line}\n`);
      if (marker !== undefined) return;
      const found = parsePortMarker(line);
      if (found !== undefined) {
        marker = found;
        onMarker?.(found);
      }
    };
    for (const stream of [child.stdout, child.stderr]) {
      const assembler = createLineAssembler(noteLine);
      stream?.on("data", (chunk: Buffer) => assembler.push(chunk.toString()));
      stream?.on("end", () => assembler.flush());
    }

    const deadline = Date.now() + (options.launchTimeoutMs ?? 300_000);
    const fail = (error: unknown): never => {
      // Only while the child is still ours: after it exits the pid can be
      // reused, and a group SIGKILL would then hit an unrelated process (the
      // same reason killProcessGroup has no positive-pid fallback).
      if (child.exitCode === null && child.signalCode === null) {
        killProcessGroup(child.pid);
      }
      pendingChildren.delete(child);
      throw new Error(
        `debug_runtime failed to start: ${error}\nRecent output:\n${formatCrashOutput(logLines)}`,
      );
    };

    // Block on the marker rather than on a port we chose. It arrives after
    // the child has bound (and can be minutes away on a cold `cargo` build),
    // so the launch timeout covers it and an early exit aborts it.
    let bound: PortMarker;
    try {
      // No `await` has run since the listeners were attached, so the marker
      // cannot have arrived yet - this promise is always the thing that waits.
      bound = await new Promise<PortMarker>((resolve, reject) => {
        const settle = () => {
          clearTimeout(timer);
          onMarker = undefined;
          child.off("exit", onExit);
          child.off("error", onSpawnError);
        };
        const onExit = () => {
          settle();
          reject(
            new Error(
              `process exited early (code ${child.exitCode}, signal ${child.signalCode}) before announcing its port`,
            ),
          );
        };
        // A failed spawn (no `cargo` on PATH) emits 'error' and never
        // 'exit', so without this the launch would wait out its whole
        // timeout - and an unhandled 'error' throws.
        const onSpawnError = (error: Error) => {
          settle();
          reject(new Error(`could not spawn cargo: ${error.message}`));
        };
        const timer = setTimeout(() => {
          settle();
          reject(
            new Error(
              `runtime never announced its port (SHOCK2QUEST_PORT) within ${options.launchTimeoutMs ?? 300_000}ms`,
            ),
          );
        }, Math.max(1_000, deadline - Date.now()));
        onMarker = (found) => {
          settle();
          resolve(found);
        };
        child.once("exit", onExit);
        child.once("error", onSpawnError);
      });
    } catch (error) {
      return fail(error);
    }
    if (bound.instanceId !== instanceId) {
      // Can only happen if something between us and the runtime rewrote the
      // marker; treat it exactly like the foreign-runtime case rather than
      // connecting to a port we cannot vouch for.
      return fail(
        new Error(
          `port marker reports a different instance (expected ${instanceId}, got ${bound.instanceId || "none"})`,
        ),
      );
    }

    const server = new GameServer(
      new HttpClient(`http://127.0.0.1:${bound.port}`),
      child,
      logLines,
      instanceId,
    );
    pendingChildren.delete(child);
    liveServers.add(server);

    try {
      await server.waitUntilReady(Math.max(1_000, deadline - Date.now()));
    } catch (error) {
      server.killProcessTree();
      server.releaseResources();
      throw new Error(
        `debug_runtime failed to start: ${error}\nRecent output:\n${formatCrashOutput(logLines)}`,
      );
    }

    return server;
  }

  /** SIGKILL the child's whole process group (cargo + the game binary). */
  private killProcessTree(): void {
    killProcessGroup(this.child?.pid);
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
        // Belt and braces on the marker's identity check: the port came from
        // our own child's marker, so a foreign runtime answering here would
        // mean the child died and something else rebound the port between the
        // marker and this request. Cross-talk with a foreign runtime looks
        // like impossible test failures, so keep failing loudly on it.
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
