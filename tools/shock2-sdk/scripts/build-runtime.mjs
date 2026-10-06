import { spawnSync } from "node:child_process";
import { constants, copyFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** Build once, then copy the executable out of Cargo's mutable target tree.
 * Another checkout may rebuild that tree while this suite is still running. */
export function buildRuntime(repoRoot) {
  const build = spawnSync("cargo", ["build", "-p", "debug_runtime", "--message-format=json-render-diagnostics"], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    maxBuffer: 64 * 1024 * 1024,
  });
  if (build.error || build.signal || build.status !== 0) {
    throw new Error(`runtime build failed: ${build.error?.message ?? build.signal ?? `exit ${build.status}`}`);
  }
  const artifacts = build.stdout.split("\n").filter(Boolean).map(line => JSON.parse(line));
  const executable = artifacts.findLast(event => event.reason === "compiler-artifact" &&
    event.target?.name === "debug_runtime" && event.executable)?.executable;
  if (!executable) throw new Error("Cargo did not report a debug_runtime executable");
  const directory = mkdtempSync(join(tmpdir(), "shock2-e2e-runtime-"));
  const cleanup = () => rmSync(directory, { recursive: true, force: true });
  try {
    const binary = join(directory, process.platform === "win32" ? "debug_runtime.exe" : "debug_runtime");
    copyFileSync(executable, binary, constants.COPYFILE_FICLONE);
    return { binary, cleanup };
  } catch (error) {
    cleanup();
    throw error;
  }
}
