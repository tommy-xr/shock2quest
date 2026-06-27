// Repeatedly runs the e2e suite to surface flakiness in timing-sensitive tests
// (e.g. the muzzle-flash tracking test, whose flash is alive only a couple of
// deterministic frames). Each run gets a unique port so leftover runtimes can't
// collide. Exits non-zero on the first failing run.
//
// Usage (build first, or rely on the npm script which runs `tsc`):
//   node scripts/reliability.mjs [count] [namePattern]
//     count        repetitions (default 10)
//     namePattern  --test-name-pattern filter (default: every e2e test)
//
//   npm run test:e2e:reliability                  # all e2e tests, 10x
//   node scripts/reliability.mjs 20 "muzzle flash" # one test, 20x
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";

const count = Number(process.argv[2] ?? 10);
const pattern = process.argv[3];

const testDir = "dist/test";
const files = readdirSync(testDir)
  .filter((f) => f.endsWith(".e2e.test.js"))
  .map((f) => join(testDir, f));

if (files.length === 0) {
  console.error(`No compiled e2e tests in ${testDir}/ - run \`npm run build\` first.`);
  process.exit(1);
}

let passed = 0;
for (let i = 1; i <= count; i++) {
  // Serialize files (--test-concurrency=1): each e2e test spawns its own heavy
  // debug runtime, so running them one at a time avoids port/resource contention.
  const args = ["--test", "--test-concurrency=1"];
  if (pattern) args.push(`--test-name-pattern=${pattern}`);
  args.push(...files);

  console.log(`=== run ${i}/${count} ===`);
  const res = spawnSync("node", args, {
    stdio: "inherit",
    env: { ...process.env, SHOCK2_E2E: "1", SHOCK2_E2E_PORT: String(8200 + i) },
  });

  if (res.status === 0) {
    passed++;
  } else {
    console.error(`\nreliability: run ${i}/${count} FAILED (${passed} passed before this)`);
    process.exit(1);
  }
}

console.log(`\nreliability: ${passed}/${count} passed`);
