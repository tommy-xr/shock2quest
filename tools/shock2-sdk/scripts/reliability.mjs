// Repeatedly runs the e2e suite to surface flakiness in timing-sensitive tests
// (e.g. the muzzle-flash tracking test, whose flash is alive only a couple of
// deterministic frames). Exits non-zero on the first failing run.
//
// Usage (build first, or rely on the npm script which runs `tsc`):
//   node scripts/reliability.mjs [count] [namePattern]
//     count        repetitions (default 10)
//     namePattern  --test-name-pattern filter (default: every e2e test)
//
//   npm run test:e2e:reliability                  # all e2e tests, 10x
//   node scripts/reliability.mjs 20 "muzzle flash" # one test, 20x
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildRuntime } from "./build-runtime.mjs";

const count = Number(process.argv[2] ?? 10);
const pattern = process.argv[3];
if (!Number.isSafeInteger(count) || count < 1) {
  console.error("reliability: count must be a positive safe integer");
  process.exit(1);
}

const testDir = "dist/test";
const files = readdirSync(testDir)
  .filter((f) => f.endsWith(".e2e.test.js"))
  .map((f) => join(testDir, f));

if (files.length === 0) {
  console.error(`No compiled e2e tests in ${testDir}/ - run \`npm run build\` first.`);
  process.exit(1);
}

if (!process.env.SHOCK2_RUNTIME_BINARY) {
  try {
    const runtime = buildRuntime(fileURLToPath(new URL("../../../", import.meta.url)));
    process.env.SHOCK2_RUNTIME_BINARY = runtime.binary;
    process.on("exit", runtime.cleanup);
  } catch (error) {
    console.error(`reliability: FAIL (${error.message})`);
    process.exit(1);
  }
}

// Count real passing test events: Node can report an empty file as a pass
// when a filter matches nothing.
const summaryDir = mkdtempSync(join(tmpdir(), "shock2-reliability-summary-"));
process.on("exit", () => rmSync(summaryDir, { recursive: true, force: true }));
const summaryPath = join(summaryDir, "summary.jsonl");
const summaryReporter = fileURLToPath(new URL("./reliability-summary.mjs", import.meta.url));
let passed = 0;
for (let i = 1; i <= count; i++) {
  // Serialize files (--test-concurrency=1): each e2e test spawns its own heavy
  // debug runtime, so running them one at a time avoids resource contention.
  // Same hang protection as run-tests.mjs: a hung runtime fails one test.
  const args = [
    "--test",
    "--test-concurrency=1",
    "--test-force-exit",
    "--test-reporter=tap",
    "--test-reporter-destination=stdout",
    `--test-reporter=${summaryReporter}`,
    `--test-reporter-destination=${summaryPath}`,
    `--test-timeout=${process.env.SHOCK2_TEST_TIMEOUT_MS || 600_000}`,
  ];
  if (pattern) args.push(`--test-name-pattern=${pattern}`);
  args.push(...files);

  console.log(`=== run ${i}/${count} ===`);
  const res = spawnSync("node", args, {
    stdio: "inherit",
    env: { ...process.env, SHOCK2_E2E: "1" },
  });

  if (res.status === 0) {
    const executed = readFileSync(summaryPath, "utf8").split("\n").filter(Boolean)
      .map(line => JSON.parse(line))
      .reduce((total, summary) => total + summary.counts.passed, 0);
    if (executed === 0) {
      console.error(`reliability: run ${i}/${count} FAILED (no passing tests executed; check filter and fixture gates)`);
      process.exit(1);
    }
    passed++;
  } else {
    console.error(`\nreliability: run ${i}/${count} FAILED (${passed} passed before this)`);
    process.exit(1);
  }
}

console.log(`\nreliability: ${passed}/${count} passed`);
