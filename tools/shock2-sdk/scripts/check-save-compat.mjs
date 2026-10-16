// Before/after save check: saves written by the base commit (default: the
// merge-base with origin/main) must load in this checkout. Builds the base
// runtime in its own worktree and target dir, writes fixture saves with it,
// then runs the save-compat unit test and release-saves e2e test on them.
//
//   npm run check:save-compat [-- <base-ref>]
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { buildRuntime } from "./build-runtime.mjs";

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const sdkRoot = join(repoRoot, "tools", "shock2-sdk");
const git = (...args) => execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
const run = (command, args, options) => {
  const result = spawnSync(command, args, { stdio: "inherit", ...options });
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")}: exit ${result.status}`);
};

const ref = process.argv[2] ?? git("merge-base", "HEAD", "origin/main");
const base = git("rev-parse", "--short", `${ref}^{commit}`);
// One base build per clone (not per worktree): it is ~2.4 GB, kept warm
// between runs at a fixed path, so runs take turns via a pid lock.
const cloneRoot = dirname(git("rev-parse", "--path-format=absolute", "--git-common-dir"));
const baseRoot = join(cloneRoot, "target", "save-compat-base");
const baseTree = join(baseRoot, "checkout");
const lock = join(baseRoot, "lock");
mkdirSync(baseRoot, { recursive: true });
try {
  writeFileSync(lock, String(process.pid), { flag: "wx" });
} catch {
  const holder = Number(readFileSync(lock, "utf8"));
  let alive = true;
  try {
    process.kill(holder, 0);
  } catch {
    alive = false;
  }
  if (alive) throw new Error(`check:save-compat is already running (pid ${holder})`);
  writeFileSync(lock, String(process.pid));
}

const fixtures = mkdtempSync(join(tmpdir(), "shock2-save-compat-"));
try {
  // An interrupted run can leave the checkout registered, unregistered or half-removed.
  try {
    git("worktree", "remove", "--force", baseTree);
  } catch {}
  rmSync(baseTree, { recursive: true, force: true });
  git("worktree", "prune");
  git("worktree", "add", "--detach", baseTree, base);

  const runtime = buildRuntime(baseTree, { ...process.env, CARGO_TARGET_DIR: join(baseRoot, "target") });
  try {
    const out = join(fixtures, `base-${base}`);
    run("node", ["dist/scripts/gen-save-fixtures.js", `base-${base}`, runtime.binary, out], { cwd: sdkRoot });
  } catch (error) {
    // This SDK drives the base runtime, so an API change can fail here too.
    throw new Error(`writing saves with ${base} failed (not a save break if the runtime API changed): ${error.message}`);
  } finally {
    runtime.cleanup();
  }
  const env = { ...process.env, SHOCK2_SAVE_FIXTURES: fixtures };
  run("cargo", ["test", "-p", "shock2vr", "--lib", "save_compat"], { cwd: repoRoot, env });
  // Unset, the runner builds this checkout rather than reusing a pinned binary.
  const e2eEnv = { ...env, SHOCK2_E2E: "1" };
  delete e2eEnv.SHOCK2_RUNTIME_BINARY;
  run("node", ["scripts/run-tests.mjs", "--test-concurrency=1", "dist/test/release-saves.e2e.test.js"], {
    cwd: sdkRoot,
    env: e2eEnv,
  });
  console.log(`save-compat: saves from ${base} load in this checkout`);
} finally {
  rmSync(fixtures, { recursive: true, force: true });
  try {
    git("worktree", "remove", "--force", baseTree);
  } catch {}
  rmSync(lock, { force: true });
}
