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

const grouped = process.platform !== "win32";
const child = spawn(process.execPath, ["--test", ...process.argv.slice(2)], {
  stdio: "inherit",
  // Give the test runner, workers and their runtimes one owned process group.
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
    escalation = setTimeout(() => signalTree("SIGKILL"), 2000);
  });
}
const res = await new Promise(resolve => {
  child.once("error", error => resolve({ error }));
  child.once("exit", (status, signal) => resolve({ status, signal }));
});
// The runner may exit before a worker's runtime. Finish the interrupted group
// before reporting a verdict, so a wrapper-only signal cannot orphan it.
if (interrupted) {
  clearTimeout(escalation);
  signalTree("SIGKILL");
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
