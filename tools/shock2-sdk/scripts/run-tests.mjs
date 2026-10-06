// Thin wrapper around `node --test <args>` that makes the run's verdict
// trustworthy for scripted consumers (#1169):
//   - exits non-zero for ANY unsuccessful run, including a runner killed by a
//     signal (mapped to a non-zero code instead of relying on shell semantics)
//   - prints one final greppable line, `shock2-sdk tests: PASS|FAIL`, so a
//     consumer that only sees a captured/tee'd log (where the exit code is
//     easily masked by pipes or trailing commands) can still gate on the result
import { spawn } from "node:child_process";

const grouped = process.platform !== "win32";
const child = spawn(process.execPath, ["--test", ...process.argv.slice(2)], {
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
