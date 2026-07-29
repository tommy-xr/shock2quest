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

test("--restart forgets the previous ledger and creates a fresh roll", () => {
  withLedger(({ run, state }) => {
    const first = run(
      "roll",
      "seed=111",
      "scenario=engineering",
      "tweak=none",
      "assets=legacy",
      "fixBranch=old-campaign-stack",
    );
    assert.equal(first.status, 0, first.stderr);
    assert.equal(run("blocker", "add", "eng1", "bug", "42", "old", "finding").status, 0);
    assert.equal(run("advance", "eng1", "frontier-001", "1,2,3", "old", "frontier").status, 0);

    const restarted = run(
      "roll",
      "--restart",
      "seed=222",
      "scenario=shodan",
      "tweak=detailed",
      "assets=legacy",
      "fixBranch=fresh-campaign-stack",
    );
    assert.equal(restarted.status, 0, restarted.stderr);
    assert.match(restarted.stdout, /forgot previous ledger.*rolled fresh campaign/i);

    const fresh = state();
    assert.equal(fresh.seed, 222);
    assert.equal(fresh.scenario.id, "shodan");
    assert.equal(fresh.tweak.id, "detailed");
    assert.equal(fresh.fix_branch, "fresh-campaign-stack");
    assert.equal(fresh.iteration, 0);
    assert.equal(fresh.frontier, null);
    assert.deepEqual(fresh.blockers, []);
    assert.deepEqual(fresh.history, []);
    assert.equal(fresh.completed, false);
    assert.equal(fresh.completion, null);
  });
});
