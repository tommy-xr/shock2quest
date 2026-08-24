import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { createServer } from "node:net";
import { test } from "node:test";

import { findRepoRoot } from "../src/server.js";

// Real-process coverage for the debug runtime's lifecycle contract:
//  - it always announces the port it actually bound (SHOCK2QUEST_PORT), so a
//    caller never has to assume - in particular with `--port 0`;
//  - it exits on its own after `--idle-timeout-secs` with no HTTP traffic, so
//    an orphaned runtime (crashed agent, killed session) stops holding ~700 MB
//    and a port forever;
//  - any request resets that idle clock.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const repoRoot = findRepoRoot(process.cwd());

interface Runtime {
  child: ChildProcess;
  lines: string[];
  /** Resolves with the first log line matching `pattern`. */
  waitForLine(pattern: RegExp, timeoutMs: number): Promise<string>;
  /** Resolves when the process exits; rejects on timeout. */
  waitForExit(timeoutMs: number): Promise<void>;
  exited(): boolean;
  kill(): void;
}

function spawnRuntime(args: string[]): Runtime {
  assert.ok(repoRoot, "could not find cargo workspace root");
  const child = spawn(
    "cargo",
    ["run", "-p", "debug_runtime", "--", "--mission", "debug_minimal", ...args],
    {
      cwd: repoRoot,
      env: { ...process.env, RUST_LOG: "debug_runtime=info" },
      stdio: ["ignore", "pipe", "pipe"],
      detached: true,
    },
  );

  const lines: string[] = [];
  const waiters: { pattern: RegExp; resolve: (line: string) => void }[] = [];
  const emit = (line: string) => {
    if (line.length === 0) return;
    lines.push(line);
    for (const waiter of [...waiters]) {
      if (waiter.pattern.test(line)) {
        waiters.splice(waiters.indexOf(waiter), 1);
        waiter.resolve(line);
      }
    }
  };
  // A chunk can split mid-line (and the marker line is exactly what we wait
  // for), so carry the tail of each stream between chunks instead of assuming
  // whole lines arrive together.
  const captureFrom = (stream: NodeJS.ReadableStream | null) => {
    let carry = "";
    stream?.on("data", (chunk: Buffer) => {
      const parts = (carry + chunk.toString()).split("\n");
      carry = parts.pop() ?? "";
      for (const line of parts) emit(line);
    });
    stream?.on("end", () => {
      emit(carry);
      carry = "";
    });
  };
  captureFrom(child.stdout);
  captureFrom(child.stderr);

  let exited = false;
  const exitWaiters: (() => void)[] = [];
  child.on("exit", () => {
    exited = true;
    for (const resolve of exitWaiters.splice(0)) resolve();
  });

  const kill = () => {
    if (exited || child.pid === undefined) return;
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      // Already gone.
    }
  };

  // These runtimes are spawned raw (not via GameServer, which needs a ready
  // instance we deliberately do not have here), so they are not covered by the
  // SDK's own signal cleanup. Without this, Ctrl-C of the test runner would
  // orphan exactly the kind of runtime this file exists to test.
  for (const signal of ["SIGINT", "SIGTERM"] as const) {
    process.once(signal, () => {
      kill();
      process.kill(process.pid, signal);
    });
  }

  return {
    child,
    lines,
    exited: () => exited,
    kill,
    waitForLine(pattern, timeoutMs) {
      const existing = lines.find((line) => pattern.test(line));
      if (existing !== undefined) return Promise.resolve(existing);
      return new Promise<string>((resolve, reject) => {
        const timer = setTimeout(
          () =>
            reject(
              new Error(
                `timed out waiting for ${pattern}\nRecent output:\n${lines.slice(-40).join("\n")}`,
              ),
            ),
          timeoutMs,
        );
        waiters.push({
          pattern,
          resolve: (line) => {
            clearTimeout(timer);
            resolve(line);
          },
        });
      });
    },
    waitForExit(timeoutMs) {
      if (exited) return Promise.resolve();
      return new Promise<void>((resolve, reject) => {
        const timer = setTimeout(
          () =>
            reject(
              new Error(
                `runtime did not exit within ${timeoutMs}ms\nRecent output:\n${lines.slice(-40).join("\n")}`,
              ),
            ),
          timeoutMs,
        );
        exitWaiters.push(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    },
  };
}

/**
 * Hold an OS-chosen port, so the runtime cannot have it. Deliberately not a
 * fixed port: the test only needs *a* taken one, and a hardcoded port is the
 * very collision this file is about.
 */
function occupyAnyPort(): Promise<{ port: number; release: () => void }> {
  return new Promise((resolve, reject) => {
    const squatter = createServer();
    squatter.once("error", reject);
    squatter.listen({ port: 0, host: "127.0.0.1" }, () => {
      const address = squatter.address();
      if (address === null || typeof address === "string") {
        reject(new Error("squatter did not report a TCP port"));
        return;
      }
      resolve({ port: address.port, release: () => squatter.close() });
    });
  });
}

function parseBoundPort(markerLine: string): number {
  const match = /SHOCK2QUEST_PORT port=(\d+)/.exec(markerLine);
  assert.ok(match, `marker line has no port=: ${markerLine}`);
  return Number(match[1]);
}

const LAUNCH_TIMEOUT_MS = 300_000;

test(
  "--port 0 binds an ephemeral port and reports it",
  { skip: !e2eEnabled, timeout: 360_000 },
  async () => {
    const runtime = spawnRuntime(["--port", "0"]);
    try {
      const marker = await runtime.waitForLine(
        /SHOCK2QUEST_PORT /,
        LAUNCH_TIMEOUT_MS,
      );
      const port = parseBoundPort(marker);
      assert.notEqual(port, 0, "must report the resolved port, not the request");
      assert.ok(port > 1024, `expected an ephemeral port, got ${port}`);
      // The marker also answers "what is running, and is it mine?".
      assert.match(marker, /pid=\d+/, "marker should carry the pid");
      assert.match(
        marker,
        /instance_id=/,
        "the instance_id field is present even when unset (hand launch)",
      );

      const health = await fetch(`http://127.0.0.1:${port}/v1/health`);
      assert.equal(health.status, 200);
      await fetch(`http://127.0.0.1:${port}/v1/shutdown`, { method: "POST" });
      await runtime.waitForExit(60_000);
    } finally {
      runtime.kill();
    }
  },
);

test(
  "an explicit --port that is taken fails loudly instead of drifting",
  { skip: !e2eEnabled, timeout: 360_000 },
  async () => {
    const { port, release } = await occupyAnyPort();
    const runtime = spawnRuntime(["--port", String(port)]);
    try {
      await runtime.waitForExit(LAUNCH_TIMEOUT_MS);
      const output = runtime.lines.join("\n");
      assert.match(output, new RegExp(`failed to bind 127\\.0\\.0\\.1:${port}`));
      assert.doesNotMatch(
        output,
        /SHOCK2QUEST_PORT /,
        "a failed bind must not announce a port",
      );
    } finally {
      runtime.kill();
      release();
    }
  },
);

test(
  "the idle watchdog exits an untouched runtime, and requests reset it",
  { skip: !e2eEnabled, timeout: 420_000 },
  async () => {
    const idleSecs = 15;
    const runtime = spawnRuntime(["--port", "0", "--idle-timeout-secs", String(idleSecs)]);
    try {
      const marker = await runtime.waitForLine(
        /SHOCK2QUEST_PORT /,
        LAUNCH_TIMEOUT_MS,
      );
      const port = parseBoundPort(marker);

      // Keep it alive well past the timeout purely by talking to it.
      const keepAliveUntil = Date.now() + idleSecs * 2000;
      while (Date.now() < keepAliveUntil) {
        const health = await fetch(`http://127.0.0.1:${port}/v1/health`);
        assert.equal(health.status, 200);
        await new Promise((resolve) => setTimeout(resolve, idleSecs * 200));
      }
      assert.ok(
        !runtime.exited(),
        "requests must reset the idle clock - the runtime should still be up",
      );

      // Now go quiet: it should exit on its own.
      await runtime.waitForExit(idleSecs * 1000 + 60_000);
      assert.match(runtime.lines.join("\n"), /SHOCK2QUEST_IDLE_EXIT /);
    } finally {
      runtime.kill();
    }
  },
);
