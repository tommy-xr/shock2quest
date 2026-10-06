import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { GameServer, findRepoRoot } from "../src/index.js";

test("an explicit runtime executable receives runtime flags without Cargo arguments", async t => {
  const directory = await mkdtemp(join(tmpdir(), "shock2-direct-launch-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const runtimeBinary = join(directory, "runtime");
  await writeFile(runtimeBinary, `#!${process.execPath}
console.log('DIRECT_LAUNCH=' + JSON.stringify(process.argv.slice(2)));
process.exit(7);
`, { mode: 0o755 });
  await assert.rejects(GameServer.launch({
    mission: "debug_minimal", runtimeBinary,
    repoRoot: findRepoRoot(import.meta.dirname),
    experimental: ["physical_held_items"], debugFlags: ["--vr"], launchTimeoutMs: 2_000,
  }), (error: Error) => {
    assert.match(error.message, /DIRECT_LAUNCH=\["--mission","debug_minimal"/);
    assert.match(error.message, /"--instance-id"/);
    assert.match(error.message, /"--experimental","physical_held_items","--vr"/);
    return true;
  });
});

test("a missing explicit executable reports its spawn error promptly", async () => {
  await assert.rejects(GameServer.launch({
    mission: "debug_minimal", runtimeBinary: "/does-not-exist/shock2-runtime",
    repoRoot: findRepoRoot(import.meta.dirname), launchTimeoutMs: 2_000,
  }), /could not spawn \/does-not-exist\/shock2-runtime/);
});
