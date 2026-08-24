import assert from "node:assert/strict";
import { test } from "node:test";

import { Game, findRepoRoot } from "../src/index.js";
import { HttpClient } from "../src/client.js";
import {
  createLineAssembler,
  formatCrashOutput,
  parsePortMarker,
} from "../src/server.js";

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

test("parsePortMarker reads the port, pid and instance id the runtime bound", () => {
  const marker = parsePortMarker(
    "SHOCK2QUEST_PORT port=54321 pid=8123 instance_id=abc-123 address=127.0.0.1:54321",
  );
  assert.deepEqual(marker, {
    port: 54321,
    instanceId: "abc-123",
    pid: 8123,
    address: "127.0.0.1:54321",
  });
});

test("parsePortMarker accepts an empty instance id (a hand launch)", () => {
  const marker = parsePortMarker(
    "SHOCK2QUEST_PORT port=8080 pid=7 instance_id= address=127.0.0.1:8080",
  );
  assert.equal(marker?.instanceId, "");
  assert.equal(marker?.port, 8080);
});

test("parsePortMarker ignores lines that are not the marker", () => {
  for (const line of [
    "INFO Starting debug runtime on port 0 with mission: debug_minimal",
    "",
    "SHOCK2QUEST_IDLE_EXIT idle_secs=1800.4 timeout_secs=1800",
  ]) {
    assert.equal(parsePortMarker(line), undefined, line);
  }
});

test("parsePortMarker rejects a marker whose port or instance id is unusable", () => {
  // The two fields the SDK acts on: half-reading either would point the client
  // at the wrong runtime.
  for (const line of [
    "SHOCK2QUEST_PORT pid=8123 instance_id= address=127.0.0.1:1",
    "SHOCK2QUEST_PORT port=notanumber pid=8123 instance_id= address=x",
    "SHOCK2QUEST_PORT port=0 pid=8123 instance_id= address=127.0.0.1:0",
    "SHOCK2QUEST_PORT port=99999 pid=8123 instance_id= address=x",
    "SHOCK2QUEST_PORT port=8080 pid=8123 address=127.0.0.1:8080",
  ]) {
    assert.equal(parsePortMarker(line), undefined, line);
  }
});

test("parsePortMarker tolerates missing pid/address, which the SDK never uses", () => {
  // Reported for humans only - a future change to them must not break every
  // launch.
  const marker = parsePortMarker("SHOCK2QUEST_PORT port=8080 instance_id=abc");
  assert.equal(marker?.port, 8080);
  assert.equal(marker?.instanceId, "abc");
  assert.equal(marker?.pid, undefined);
  assert.equal(marker?.address, undefined);
});

test("a marker split across stdout chunks is still read as one line", () => {
  const lines: string[] = [];
  const assembler = createLineAssembler((line) => lines.push(line));
  // A chunk boundary can fall anywhere, including mid-marker.
  assembler.push("INFO warming up\nSHOCK2QUEST_PORT port=543");
  assert.equal(
    lines.filter((line) => parsePortMarker(line) !== undefined).length,
    0,
    "half a marker must not parse",
  );
  assembler.push("21 pid=8123 instance_id=abc address=127.0.0.1:54321\nINFO ready\n");

  const markers = lines.map(parsePortMarker).filter((m) => m !== undefined);
  assert.equal(markers.length, 1);
  assert.equal(markers[0]?.port, 54321);
  assert.equal(markers[0]?.instanceId, "abc");
});

test("createLineAssembler emits a final unterminated line on flush", () => {
  const lines: string[] = [];
  const assembler = createLineAssembler((line) => lines.push(line));
  assembler.push("tail with no newline");
  assert.deepEqual(lines, []);
  assembler.flush();
  assert.deepEqual(lines, ["tail with no newline"]);
  assembler.flush();
  assert.deepEqual(lines, ["tail with no newline"], "flush is not repeatable");
});
