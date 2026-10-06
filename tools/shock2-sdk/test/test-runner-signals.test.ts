import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const runner = fileURLToPath(new URL("../../scripts/run-tests.mjs", import.meta.url));
function environment() {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.SHOCK2_E2E;
  return env;
}

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  test(`wrapper ${signal} stops test workers and their runtime children`, {
    skip: process.platform === "win32", timeout: 10_000,
  }, async t => {
    const root = mkdtempSync(join(tmpdir(), "shock2-runner-signals-"));
    const pids = join(root, "pids");
    t.after(() => {
      if (existsSync(pids)) for (const pid of readFileSync(pids, "utf8").trim().split("\n")) {
        try { process.kill(Number(pid), "SIGKILL"); } catch { /* already exited */ }
      }
      rmSync(root, { recursive: true, force: true });
    });
    const runtime = join(root, "runtime.cjs");
    writeFileSync(runtime, `require('node:fs').appendFileSync(${JSON.stringify(pids)}, process.pid+'\\n');
console.log('AUDIT_RUNTIME_READY');
setInterval(() => {}, 1000);
`);
    const fixture = join(root, "fixture.test.cjs");
    writeFileSync(fixture, `require('node:fs').appendFileSync(${JSON.stringify(pids)}, process.pid+'\\n');
require('node:child_process').spawn(process.execPath, [${JSON.stringify(runtime)}], {stdio:'inherit'});
require('node:test').test('ongoing runtime', async () => new Promise(() => {}));
`);
    const child = spawn(process.execPath, [runner, fixture], { env: environment() });
    t.after(() => child.kill("SIGKILL"));
    let output = "";
    const exited = new Promise<number | null>(resolve => child.once("exit", resolve));
    await new Promise<void>((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", () => reject(new Error(`runner exited before ready: ${output}`)));
      child.stdout.on("data", chunk => {
        output += chunk;
        if (output.includes("AUDIT_RUNTIME_READY")) resolve();
      });
      child.stderr.on("data", chunk => { output += chunk; });
    });
    assert.equal(readFileSync(pids, "utf8").trim().split("\n").length, 2);
    child.kill(signal); // Signal only the wrapper, as an automation supervisor does.
    assert.notEqual(await exited, 0);
    assert.match(output, /shock2-sdk tests: FAIL \(runner killed by SIG(INT|TERM)\)/);
    for (const pid of readFileSync(pids, "utf8").trim().split("\n")) {
      // A killed descendant may briefly await reaping; a zombie cannot retain
      // runtime resources. Check process state rather than kill(pid, 0).
      const status = spawnSync("ps", ["-o", "stat=", "-p", pid], { encoding: "utf8" }).stdout.trim();
      assert.ok(status === "" || status.startsWith("Z"), `child ${pid} remains alive: ${status}`);
    }
  });
}

test("test wrapper preserves successful and failed verdicts", t => {
  const root = mkdtempSync(join(tmpdir(), "shock2-runner-verdict-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const fail of [false, true]) {
    const fixture = join(root, "fixture.test.cjs");
    writeFileSync(fixture, fail ? "throw new Error('expected fixture failure');" : "");
    const result = spawnSync(process.execPath, [runner, fixture], { env: environment(), encoding: "utf8" });
    assert.equal(result.status, fail ? 1 : 0, result.stdout + result.stderr);
    assert.match(result.stdout, fail ? /shock2-sdk tests: FAIL/ : /shock2-sdk tests: PASS/);
  }
});
