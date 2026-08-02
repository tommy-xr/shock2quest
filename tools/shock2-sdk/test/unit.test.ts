import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { test } from "node:test";

import { Game, findRepoRoot } from "../src/index.js";
import { HttpClient } from "../src/client.js";
import { formatCrashOutput } from "../src/server.js";

test("formatCrashOutput keeps the panic message and backtrace together", () => {
  const lines = [
    ...Array.from({ length: 50 }, (_, i) => `INFO startup line ${i}`),
    "INFO starting level load",
    "thread 'main' panicked at dark/src/mission/path_database.rs:197:73:",
    "called `Result::unwrap()` on an `Err` value",
    "stack backtrace:",
    "   0: std::panicking::begin_panic_handler",
    "   1: dark::mission::path_database::PathDatabase::read",
  ];
  const output = formatCrashOutput(lines);
  assert.match(output, /starting level load/, "includes context before the panic");
  assert.match(output, /panicked at/);
  assert.match(output, /PathDatabase::read/, "includes backtrace frames");
  assert.ok(!output.includes("startup line 10"), "drops unrelated early output");
});

test("formatCrashOutput falls back to the tail when there is no panic", () => {
  const lines = Array.from({ length: 100 }, (_, i) => `line ${i}`);
  const output = formatCrashOutput(lines);
  assert.ok(output.includes("line 99"));
  assert.ok(!output.includes("line 50"));
});

test("findRepoRoot locates the cargo workspace from the SDK directory", () => {
  const root = findRepoRoot(import.meta.dirname);
  assert.ok(root, "expected to find a workspace root above the SDK");
  assert.ok(!root.includes("shock2-sdk"), "workspace root should be above the SDK package");
});

test("waitFor resolves with the first truthy value", async () => {
  const game = new Game(new HttpClient("http://unused.invalid"));
  let calls = 0;
  const value = await game.waitFor(() => (++calls >= 3 ? "ready" : undefined), {
    intervalMs: 1,
  });
  assert.equal(value, "ready");
  assert.equal(calls, 3);
});

test("waitFor times out with a descriptive error", async () => {
  const game = new Game(new HttpClient("http://unused.invalid"));
  await assert.rejects(
    game.waitFor(() => false, {
      timeoutMs: 30,
      intervalMs: 5,
      description: "the impossible",
    }),
    /Timed out after 30ms waiting for the impossible/,
  );
});

test("findFreePort skips ports that are already bound", async () => {
  const { findFreePort } = await import("../src/server.js");
  const { createServer } = await import("node:net");

  // Occupy a port, then ask for it: the next one up should come back.
  const blocker = createServer();
  await new Promise<void>((resolve) =>
    blocker.listen({ port: 0, host: "127.0.0.1" }, resolve),
  );
  const address = blocker.address();
  assert.ok(address && typeof address === "object");
  const taken = address.port;
  try {
    const free = await findFreePort(taken);
    assert.ok(free > taken, `expected a port above ${taken}, got ${free}`);
  } finally {
    await new Promise((resolve) => blocker.close(resolve));
  }
});

test("reapStaleRuntimeOnPort kills a debug_runtime bound to the port", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");
  const killed: Array<{ pid: number; signal: string }> = [];

  const result = await reapStaleRuntimeOnPort(8080, {
    findProcesses: async (port) => {
      assert.equal(port, 8080);
      return [{ pid: 4242, command: "target/debug/debug_runtime --mission medsci1.mis --port 8080" }];
    },
    kill: (pid, signal) => killed.push({ pid, signal }),
  });

  assert.deepEqual(result, [4242]);
  assert.deepEqual(killed, [{ pid: 4242, signal: "SIGKILL" }]);
});

test("reapStaleRuntimeOnPort never kills a process that isn't debug_runtime", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");
  const killed: number[] = [];

  const result = await reapStaleRuntimeOnPort(8080, {
    findProcesses: async () => [
      { pid: 111, command: "com.docker.backend --some-flag" },
      { pid: 222, command: "node dist/some-other-server.js --port 8080" },
    ],
    kill: (pid) => killed.push(pid),
  });

  assert.deepEqual(result, []);
  assert.deepEqual(killed, []);
});

test("reapStaleRuntimeOnPort only kills the candidate whose args exactly name --port <port>", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");
  const killed: number[] = [];

  // Two debug_runtime processes turn up (e.g. a stale entry mid-lsof-vs-ps
  // race) - only the one that actually names this port should be killed.
  const result = await reapStaleRuntimeOnPort(9001, {
    findProcesses: async () => [
      { pid: 1, command: "target/debug/debug_runtime --mission medsci1.mis --port 9002" },
      { pid: 2, command: "target/debug/debug_runtime --mission earth.mis --port 9001" },
    ],
    kill: (pid) => killed.push(pid),
  });

  assert.deepEqual(result, [2]);
  assert.deepEqual(killed, [2]);
});

test("reapStaleRuntimeOnPort rejects substring look-alikes (executable name and port)", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");
  const killed: number[] = [];

  const result = await reapStaleRuntimeOnPort(808, {
    findProcesses: async () => [
      // Not our binary, despite containing "debug_runtime" as a substring.
      { pid: 1, command: "/opt/tools/debug_runtime_proxy --port 808" },
      // Right binary, but "--port 8080" is not the literal token "808".
      { pid: 2, command: "target/debug/debug_runtime --mission medsci1.mis --port 8080" },
    ],
    kill: (pid) => killed.push(pid),
  });

  assert.deepEqual(result, [], "neither look-alike should be treated as a match");
  assert.deepEqual(killed, []);
});

test("reapStaleRuntimeOnPort is a no-op when nothing is listening", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");
  const killed: number[] = [];

  const result = await reapStaleRuntimeOnPort(8080, {
    findProcesses: async () => [],
    kill: (pid) => killed.push(pid),
  });

  assert.deepEqual(result, []);
  assert.deepEqual(killed, []);
});

test("reapStaleRuntimeOnPort tolerates a kill failing (process already gone)", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");

  const result = await reapStaleRuntimeOnPort(8080, {
    findProcesses: async () => [
      { pid: 4242, command: "target/debug/debug_runtime --mission medsci1.mis --port 8080" },
    ],
    kill: () => {
      throw new Error("ESRCH: no such process");
    },
  });

  assert.deepEqual(result, [], "a failed kill isn't reported as killed");
});

test("reapStaleRuntimeOnPort swallows discovery errors", async () => {
  const { reapStaleRuntimeOnPort } = await import("../src/server.js");

  const result = await reapStaleRuntimeOnPort(8080, {
    findProcesses: async () => {
      throw new Error("lsof not found");
    },
  });

  assert.deepEqual(result, []);
});

test("GameServer.launch reaps only the requested launch port, never a fallback port", async () => {
  const { GameServer, reapHooks } = await import("../src/server.js");
  const requestedPorts: number[] = [];
  const originalReap = reapHooks.reapStaleRuntimeOnPort;

  // Stub out the reap hook to observe what port(s) launch() asks it to
  // check, without spawning a real debug_runtime. This exercises the option
  // plumbing added for #786: `reapStale` gating the call, and the call
  // always targeting the literal requested port hint - never `hint + attempt`
  // from the bind-race retry loop.
  reapHooks.reapStaleRuntimeOnPort = async (port: number) => {
    requestedPorts.push(port);
    return [];
  };
  try {
    // repoRoot points at a real directory with no Cargo.toml, so `cargo run`
    // fails near-instantly ("could not find Cargo.toml") instead of actually
    // compiling/running debug_runtime - fast and side-effect-free, while
    // still exercising the real launch()/launchOnce() code path around the
    // reap call.
    await assert.rejects(
      GameServer.launch({
        mission: "medsci1.mis",
        port: 47001,
        launchTimeoutMs: 2000,
        repoRoot: tmpdir(),
      }),
    );
  } finally {
    reapHooks.reapStaleRuntimeOnPort = originalReap;
  }

  assert.deepEqual(requestedPorts, [47001], "reaps exactly the requested port, once");
});

test("GameServer.launch skips reaping when reapStale is false", async () => {
  const { GameServer, reapHooks } = await import("../src/server.js");
  const requestedPorts: number[] = [];
  const originalReap = reapHooks.reapStaleRuntimeOnPort;

  reapHooks.reapStaleRuntimeOnPort = async (port: number) => {
    requestedPorts.push(port);
    return [];
  };
  try {
    await assert.rejects(
      GameServer.launch({
        mission: "medsci1.mis",
        port: 47002,
        launchTimeoutMs: 2000,
        repoRoot: tmpdir(),
        reapStale: false,
      }),
    );
  } finally {
    reapHooks.reapStaleRuntimeOnPort = originalReap;
  }

  assert.deepEqual(requestedPorts, [], "reapStale: false must not call the reap hook at all");
});
