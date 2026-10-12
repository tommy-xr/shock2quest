import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

test("reliability rejects zero, fractional and non-finite counts without running tests", t => {
  const root = mkdtempSync(join(tmpdir(), "shock2-reliability-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const script = fileURLToPath(new URL("../../scripts/reliability.mjs", import.meta.url));
  mkdirSync(join(root, "dist/test"), { recursive: true });
  const marker = join(root, "runs");
  writeFileSync(join(root, "dist/test/fixture.e2e.test.js"),
    `require('node:test').test('fixture', () => require('node:fs').appendFileSync(${JSON.stringify(marker)}, 'run\\n'));`);
  const env: NodeJS.ProcessEnv = { ...process.env, SHOCK2_RUNTIME_BINARY: "unused-fixture-runtime" };
  delete env.NODE_TEST_CONTEXT;
  for (const count of ["0", "-1", "1.5", "NaN", "Infinity", "9007199254740992"]) {
    const result = spawnSync(process.execPath, [script, count], { cwd: root, env, encoding: "utf8" });
    assert.equal(result.status, 1, `invalid count ${count}: ${result.stdout}${result.stderr}`);
    assert.match(result.stderr, /count must be a positive safe integer/);
  }
  const result = spawnSync(process.execPath, [script, "2"], { cwd: root, env, encoding: "utf8" });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /reliability: 2\/2 passed/);
  assert.equal(readFileSync(marker, "utf8"), "run\nrun\n", "only the requested positive runs execute");
});


test("reliability rejects unmatched filters and runs containing only skipped or TODO tests", t => {
  const root = mkdtempSync(join(tmpdir(), "shock2-reliability-empty-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const script = fileURLToPath(new URL("../../scripts/reliability.mjs", import.meta.url));
  mkdirSync(join(root, "dist/test"), { recursive: true });
  const fixture = join(root, "dist/test/fixture.e2e.test.js");
  const env: NodeJS.ProcessEnv = { ...process.env, SHOCK2_RUNTIME_BINARY: "unused-fixture-runtime" };
  delete env.NODE_TEST_CONTEXT;
  for (const [source, pattern] of [
    ["require('node:test').test('real case', () => {});", "unmatched"],
    ["require('node:test').test('skipped case', {skip:true}, () => {});", "case"],
    ["require('node:test').test('TODO case', {todo:true}, () => {});", "case"],
  ]) {
    writeFileSync(fixture, source);
    const result = spawnSync(process.execPath, [script, "1", pattern], { cwd: root, env, encoding: "utf8" });
    assert.equal(result.status, 1, result.stdout + result.stderr);
    assert.match(result.stderr, /no passing tests executed/);
    assert.doesNotMatch(result.stdout, /reliability: 1\/1 passed/);
  }
  writeFileSync(fixture, "require('node:test').test('real case', () => {});");
  const result = spawnSync(process.execPath, [script, "1", "real case"], { cwd: root, env, encoding: "utf8" });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /reliability: 1\/1 passed/);
});
