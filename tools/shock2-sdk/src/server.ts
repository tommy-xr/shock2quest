import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

import { HttpClient } from "./client.js";
import { Game } from "./game.js";

export interface LaunchOptions {
  /** Mission file or debug scene, e.g. "medsci1.mis" or "debug_minimal". */
  mission: string;
  /** Port for the debug runtime HTTP server (default 8080). */
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
  ) {
    super(client);
  }

  /** Recent stdout/stderr from the spawned runtime (ring buffer). */
  logs(): string[] {
    return [...this.logLines];
  }

  /** Connect to an already-running debug runtime. shutdown() will stop it; dispose will not spawn-kill anything. */
  static async connect(baseUrl = "http://127.0.0.1:8080"): Promise<GameServer> {
    const server = new GameServer(new HttpClient(baseUrl), undefined, []);
    await server.health();
    return server;
  }

  /** Spawn a debug runtime via `cargo run -p debug_runtime` and wait for it to be ready. */
  static async launch(options: LaunchOptions): Promise<GameServer> {
    const port = options.port ?? 8080;
    const repoRoot = options.repoRoot ?? findRepoRoot(process.cwd());
    if (repoRoot === undefined) {
      throw new Error(
        "Could not find cargo workspace root; pass repoRoot explicitly",
      );
    }

    const args = [
      "run",
      "-p",
      "debug_runtime",
      "--",
      "--mission",
      options.mission,
      "--port",
      String(port),
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
    );

    try {
      await server.waitUntilReady(options.launchTimeoutMs ?? 300_000);
    } catch (error) {
      child.kill("SIGKILL");
      throw new Error(
        `debug_runtime failed to start: ${error}\nRecent output:\n${formatCrashOutput(logLines)}`,
      );
    }

    return server;
  }

  private async waitUntilReady(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      if (this.child && this.child.exitCode !== null) {
        throw new Error(`process exited early with code ${this.child.exitCode}`);
      }
      try {
        // /v1/info round-trips through the game loop's command channel, so
        // it only succeeds once the game thread is actually running. (The
        // HTTP server starts before - and can outlive - the game thread,
        // so /v1/health alone would accept a runtime whose game thread
        // crashed during mission load.)
        await this.info();
        return;
      } catch {
        if (Date.now() >= deadline) {
          throw new Error(`server not ready after ${timeoutMs}ms`);
        }
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }
    }
  }

  /** Gracefully stop the runtime; escalates to SIGKILL if it doesn't exit. */
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
        child.kill("SIGKILL");
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
