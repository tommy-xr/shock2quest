import assert from "node:assert/strict";
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
