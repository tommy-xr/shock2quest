import { type ChildProcess, spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { dirname, join } from "node:path";

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
    if (await portIsFree(port)) {
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
  ) {
    super(client);
  }

  /** Recent stdout/stderr from the spawned runtime (ring buffer). */
  logs(): string[] {
    return [...this.logLines];
  }

  /** Connect to an already-running debug runtime. shutdown() will stop it; dispose will not spawn-kill anything. */
  static async connect(baseUrl = "http://127.0.0.1:8080"): Promise<GameServer> {
    const server = new GameServer(new HttpClient(baseUrl), undefined, [], undefined);
    await server.health();
    return server;
  }

  /** Spawn a debug runtime via `cargo run -p debug_runtime` and wait for it to be ready. */
  static async launch(options: LaunchOptions): Promise<GameServer> {
    const port = await findFreePort(options.port ?? 8080);
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
    );

    try {
      await server.waitUntilReady(options.launchTimeoutMs ?? 300_000);
    } catch (error) {
      server.killProcessTree();
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
      try {
        this.child?.kill("SIGKILL");
      } catch {
        // Already gone.
      }
    }
  }

  private async waitUntilReady(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      if (this.child && this.child.exitCode !== null) {
        throw new Error(`process exited early with code ${this.child.exitCode}`);
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
    try {
      await super.shutdown();
    } catch {
      // Server may already be down; fall through to process cleanup.
    }
    const child = this.child;
    if (child === undefined || child.exitCode !== null) {
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
  }

  async [Symbol.asyncDispose](): Promise<void> {
    await this.shutdown();
  }
}
