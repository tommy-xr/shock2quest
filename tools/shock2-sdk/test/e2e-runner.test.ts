import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test, type TestContext } from "node:test";
import { fileURLToPath } from "node:url";

function fixture(t: TestContext, fail = false) {
  const root = mkdtempSync(join(tmpdir(), "shock2-runner-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const artifact = join(root, "artifact");
  writeFileSync(artifact, "original", { mode: 0o755 });
  const count = join(root, "builds");
  const cargo = join(root, "cargo");
  writeFileSync(cargo, `#!${process.execPath}
const fs = require('node:fs');
fs.appendFileSync(${JSON.stringify(count)}, JSON.stringify(process.argv.slice(2))+'\\n');
${fail ? "process.exit(9);" : `console.log(JSON.stringify({reason:'compiler-artifact',target:{name:'debug_runtime'},executable:${JSON.stringify(artifact)}}));`}
`);
  chmodSync(cargo, 0o755);
  const env: NodeJS.ProcessEnv = { ...process.env, SHOCK2_E2E: "1", PATH: root + delimiter + process.env.PATH };
  delete env.SHOCK2_RUNTIME_BINARY;
  delete env.NODE_TEST_CONTEXT;
  return { root, artifact, count, env };
}

test("E2E runner builds once and pins the executable across test files", t => {
  const { root, artifact, count, env } = fixture(t);
  const paths = ["a", "b"].map(name => {
    const path = join(root, `${name}.test.cjs`);
    writeFileSync(path, `
const assert = require('node:assert/strict');
const fs = require('node:fs');
const binary = process.env.SHOCK2_RUNTIME_BINARY;
assert.notEqual(binary, ${JSON.stringify(artifact)});
assert.equal(fs.readFileSync(binary, 'utf8'), 'original');
fs.writeFileSync(${JSON.stringify(artifact)}, 'rebuilt elsewhere');
fs.appendFileSync(${JSON.stringify(join(root, "snapshots"))}, binary+'\\n');
`);
    return path;
  });
  const result = spawnSync(process.execPath, [
    fileURLToPath(new URL("../../scripts/run-tests.mjs", import.meta.url)),
    "--test-concurrency=1", ...paths,
  ], { env, encoding: "utf8" });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /shock2-sdk tests: PASS/);
  assert.deepEqual(readFileSync(count, "utf8").trim().split("\n").map(line => JSON.parse(line)),
    [["build", "-p", "debug_runtime", "--message-format=json-render-diagnostics"]]);
  const snapshots = readFileSync(join(root, "snapshots"), "utf8").trim().split("\n");
  assert.equal(snapshots.length, 2);
  assert.equal(snapshots[0], snapshots[1]);
  assert.equal(existsSync(snapshots[0]), false, "the run's private executable is cleaned up");
});

test("E2E runner reports build failure without starting tests", t => {
  const { root, env } = fixture(t, true);
  const marker = join(root, "ran");
  const path = join(root, "must-not-run.test.cjs");
  writeFileSync(path, `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ran');`);
  const result = spawnSync(process.execPath, [
    fileURLToPath(new URL("../../scripts/run-tests.mjs", import.meta.url)), path,
  ], { env, encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /shock2-sdk tests: FAIL \(runtime build failed: exit 9\)/);
  assert.equal(existsSync(marker), false);
});
