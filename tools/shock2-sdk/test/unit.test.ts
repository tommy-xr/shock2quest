import assert from "node:assert/strict";
import { test } from "node:test";

import { Game, findRepoRoot } from "../src/index.js";
import { HttpClient } from "../src/client.js";

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
