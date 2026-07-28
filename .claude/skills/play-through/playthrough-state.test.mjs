import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const SCRIPT = fileURLToPath(new URL("./playthrough-state.mjs", import.meta.url));

function withLedger(runTest) {
  const dir = mkdtempSync(join(tmpdir(), "playthrough-state-"));
  const stateFile = join(dir, "state.json");
  const run = (...args) =>
    spawnSync(process.execPath, [SCRIPT, ...args], {
      encoding: "utf8",
      env: { ...process.env, PT_STATE: stateFile },
    });
  const state = () => JSON.parse(readFileSync(stateFile, "utf8"));

  try {
    runTest({ run, state });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("a blocker pauses autonomous play only after its third failed fix", () => {
  withLedger(({ run, state }) => {
    assert.equal(run("init", "order=rec1,rec2").status, 0);
    assert.equal(
      run("blocker", "add", "rec1", "feature-gap", "42", "broken", "objective").status,
      0,
    );

    const bypass = run("blocker", "set", "42", "failed");
    assert.notEqual(bypass.status, 0);
    assert.match(bypass.stderr, /use 'blocker fail/);

    for (let attempt = 1; attempt <= 2; attempt += 1) {
      const failure = run("blocker", "fail", "42", "100");
      assert.equal(failure.status, 0, failure.stderr);
      assert.equal(state().blockers[0].fix_failures, attempt);
      assert.equal(state().blockers[0].status, "reworking");
      assert.doesNotMatch(run("show").stdout, /PAUSE/);
    }

    const thirdFailure = run("blocker", "fail", "42", "100");
    assert.equal(thirdFailure.status, 0, thirdFailure.stderr);
    assert.equal(state().blockers[0].fix_failures, 3);
    assert.equal(state().blockers[0].status, "failed");
    assert.match(run("show").stdout, /PAUSE.*failed 3 times.*needs a human/s);
  });
});

test("campaign completion is persisted and surfaced as the terminal action", () => {
  withLedger(({ run, state }) => {
    assert.equal(run("init", "order=rec1,rec2").status, 0);

    const premature = run("complete", "rec1", "not", "at", "the", "end");
    assert.notEqual(premature.status, 0);
    assert.match(premature.stderr, /final mission.*rec2/);

    const completed = run("complete", "rec2", "campaign", "exit", "reached");
    assert.equal(completed.status, 0, completed.stderr);
    assert.equal(state().completed, true);
    assert.equal(state().completion.level, "rec2");
    assert.equal(state().completion.note, "campaign exit reached");
    assert.match(run("show").stdout, /COMPLETE.*campaign exit reached/s);
  });
});
