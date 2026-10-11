// Thin wrapper around `node --test <args>` that makes the run's verdict
// trustworthy for scripted consumers (#1169):
//   - exits non-zero for ANY unsuccessful run, including a runner killed by a
//     signal (mapped to a non-zero code instead of relying on shell semantics)
//   - prints one final greppable line, `shock2-sdk tests: PASS|FAIL`, so a
//     consumer that only sees a captured/tee'd log (where the exit code is
//     easily masked by pipes or trailing commands) can still gate on the result
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { buildRuntime } from "./build-runtime.mjs";

let runtime;
if (process.env.SHOCK2_E2E === "1" && !process.env.SHOCK2_RUNTIME_BINARY) {
  try {
    runtime = buildRuntime(fileURLToPath(new URL("../../../", import.meta.url)));
    process.env.SHOCK2_RUNTIME_BINARY = runtime.binary;
    process.on("exit", runtime.cleanup);
  } catch (error) {
    console.error(`shock2-sdk tests: FAIL (${error.message})`);
    process.exit(1);
  }
}

// node's own default is no timeout, so one hung runtime would stall a serial
// suite forever. Apply a generous default to tests that set no timeout of
// their own; explicit per-test timeouts and a caller's --test-timeout win.
// A timed-out test's body never settles, so its `await using` never disposes
// the runtime and the open handles would keep the file alive anyway:
// --test-force-exit ends the file once its tests are done, and the SDK's exit
// hook kills the runtimes it still owns.
const args = ["--test-force-exit", ...process.argv.slice(2)];
if (!args.some(arg => arg === "--test-timeout" || arg.startsWith("--test-timeout="))) {
  args.unshift(`--test-timeout=${process.env.SHOCK2_TEST_TIMEOUT_MS || 600_000}`);
}

const grouped = process.platform !== "win32";
const child = spawn(process.execPath, ["--test", ...args], {
  stdio: "inherit",
  // Give the runner and workers one owned group. SDK runtimes are detached;
  // their worker's signal hook shuts down those separately owned groups.
  detached: grouped,
});
let interrupted;
let escalation;
function signalTree(signal) {
  if (!child.pid) return;
  try {
    if (grouped) process.kill(-child.pid, signal);
    else child.kill(signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    if (interrupted) return;
    interrupted = signal;
    signalTree(signal);
    escalation = new Promise(resolve => setTimeout(() => {
      signalTree("SIGKILL");
      resolve();
    }, 2000));
  });
}
const res = await new Promise(resolve => {
  child.once("error", error => resolve({ error }));
  child.once("exit", (status, signal) => resolve({ status, signal }));
});
// A runner may exit before workers have handled the signal. Preserve the grace
// period so their SDK hooks can kill detached runtimes before we kill workers.
if (interrupted) {
  await escalation;
}
let code;
if (res.error) {
  console.error(`shock2-sdk tests: FAIL (could not run node --test: ${res.error.message})`);
  code = 1;
} else if (interrupted || res.signal) {
  console.error(`shock2-sdk tests: FAIL (runner killed by ${interrupted ?? res.signal})`);
  code = 1;
} else {
  code = res.status ?? 1;
  console.log(`shock2-sdk tests: ${code === 0 ? "PASS" : `FAIL (exit code ${code})`}`);
}
process.exit(code);
