import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { ANNIVERSARY_ASSETS, PRESENTATION_MODES, roll } from "./scenarios.mjs";

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
  const writeState = (value) => writeFileSync(stateFile, `${JSON.stringify(value, null, 2)}\n`);

  try {
    runTest({ run, state, writeState });
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
      "presentation=flat",
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
      "presentation=vr",
      "fixBranch=fresh-campaign-stack",
    );
    assert.equal(restarted.status, 0, restarted.stderr);
    assert.match(restarted.stdout, /forgot previous ledger.*rolled fresh campaign/i);

    const fresh = state();
    assert.equal(fresh.seed, 222);
    assert.equal(fresh.scenario.id, "shodan");
    assert.equal(fresh.tweak.id, "detailed");
    assert.equal(fresh.assets.id, "25th");
    assert.equal(fresh.presentation.id, "vr");
    assert.equal(fresh.fix_branch, "fresh-campaign-stack");
    assert.equal(fresh.iteration, 0);
    assert.equal(fresh.frontier, null);
    assert.deepEqual(fresh.blockers, []);
    assert.deepEqual(fresh.history, []);
    assert.equal(fresh.completed, false);
    assert.equal(fresh.completion, null);
  });
});

test("campaign rolls always use 25th Anniversary assets and persist presentation", () => {
  withLedger(({ run, state }) => {
    const rolled = run(
      "roll",
      "seed=42",
      "scenario=hydroponics",
      "tweak=none",
      "presentation=vr",
    );
    assert.equal(rolled.status, 0, rolled.stderr);
    assert.equal(state().assets.id, "25th");
    assert.equal(state().presentation.id, "vr");

    const shown = run("show");
    assert.equal(shown.status, 0, shown.stderr);
    assert.match(shown.stdout, /DARK_ASSET_PATH=.*ss2-25th.*in VR with --vr/s);

    const legacyOverride = run("roll", "--force", "assets=legacy");
    assert.notEqual(legacyOverride.status, 0);
    assert.match(legacyOverride.stderr, /every playtest uses the 25th Anniversary/i);
  });
});

test("the presentation roll is a uniform flat/VR choice with fixed assets", () => {
  assert.deepEqual(
    PRESENTATION_MODES.map((mode) => mode.id),
    ["flat", "vr"],
  );

  const seenModes = new Set();
  for (let seed = 0; seed < 20; seed += 1) {
    const campaign = roll({ seed });
    assert.equal(campaign.assets, ANNIVERSARY_ASSETS);
    assert.ok(PRESENTATION_MODES.includes(campaign.presentation));
    seenModes.add(campaign.presentation.id);
  }
  assert.deepEqual([...seenModes].sort(), ["flat", "vr"]);
});

test("--vr forces new and resumed campaigns without resetting progress", () => {
  withLedger(({ run, state }) => {
    const created = run(
      "roll",
      "seed=7",
      "scenario=engineering",
      "tweak=none",
      "presentation=flat",
    );
    assert.equal(created.status, 0, created.stderr);
    assert.equal(run("advance", "eng1", "frontier-007", "4,5,6").status, 0);

    const overridden = run("roll", "--vr");
    assert.equal(overridden.status, 0, overridden.stderr);
    assert.match(overridden.stdout, /overrode presentation to VR/i);
    assert.equal(state().presentation.id, "vr");
    assert.equal(state().iteration, 1);
    assert.equal(state().frontier.save, "frontier-007");

    const restarted = run(
      "roll",
      "--restart",
      "--vr",
      "seed=8",
      "scenario=shodan",
      "tweak=none",
    );
    assert.equal(restarted.status, 0, restarted.stderr);
    assert.equal(state().presentation.id, "vr");
    assert.equal(state().iteration, 0);
    assert.equal(state().frontier, null);
  });
});

test("show migrates an existing legacy-assets campaign without losing its frontier", () => {
  withLedger(({ run, state, writeState }) => {
    writeState({
      iteration: 3,
      mission_order: ["eng1", "eng2"],
      fix_branch: "playthrough-fixes",
      frontier: { level: "eng1", save: "frontier-old", position: [1, 2, 3] },
      blockers: [],
      history: [],
      completed: false,
      completion: null,
      seed: 99,
      scenario: { id: "engineering", name: "Engineering", goal: "Enable power" },
      tweak: { id: "none", name: "None", instructions: "No special constraint" },
      assets: { id: "legacy", name: "Legacy assets", path: "/old/assets" },
    });

    const shown = run("show");
    assert.equal(shown.status, 0, shown.stderr);
    assert.equal(state().assets.id, "25th");
    assert.ok(["flat", "vr"].includes(state().presentation.id));
    assert.equal(state().iteration, 3);
    assert.equal(state().frontier.save, "frontier-old");
  });
});
