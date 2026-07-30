import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const selector = fileURLToPath(
  new URL("./select-open-issues.mjs", import.meta.url),
);

function issue(number, author = "owner") {
  return {
    number,
    title: `Issue ${number}`,
    html_url: `https://github.com/owner/repo/issues/${number}`,
    user: { login: author },
    labels: [],
    assignees: [],
  };
}

function pullRequest(number, body) {
  return {
    number,
    title: `PR ${number}`,
    html_url: `https://github.com/owner/repo/pull/${number}`,
    body,
    pull_request: {},
  };
}

test("samples only owner issues without an active closing pull request", () => {
  const temporaryDirectory = mkdtempSync(
    path.join(tmpdir(), "select-open-issues-"),
  );
  const fakeGh = path.join(temporaryDirectory, "gh");
  writeFileSync(
    fakeGh,
    `#!/usr/bin/env node
if (process.argv[2] !== "api") {
  process.stderr.write("unexpected gh command");
  process.exit(2);
}
process.stdout.write(process.env.FAKE_GH_RESPONSE);
`,
  );
  chmodSync(fakeGh, 0o755);

  try {
    const response = [
      [
        issue(10),
        issue(11),
        issue(12),
        issue(13),
        issue(14),
        issue(15, "community-member"),
        pullRequest(90, "Fixes #11"),
        pullRequest(91, "Related context: #10"),
        pullRequest(92, "Resolves owner/repo#12"),
        pullRequest(
          93,
          "Closes https://github.com/owner/repo/issues/13",
        ),
        pullRequest(94, "Fixes someone/else#14"),
        pullRequest(95, "Fixes #11"),
      ],
    ];
    const result = spawnSync(
      process.execPath,
      [selector, "5", "--repo", "owner/repo", "--seed", "active-pr-test"],
      {
        encoding: "utf8",
        env: {
          ...process.env,
          FAKE_GH_RESPONSE: JSON.stringify(response),
          PATH: `${temporaryDirectory}:${process.env.PATH}`,
        },
      },
    );

    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(result.stdout);
    assert.equal(output.totalOpenIssues, 6);
    assert.equal(output.openIssues, 5);
    assert.equal(output.available, 2);
    assert.deepEqual(
      output.excludedNonOwnerIssues.map(({ number, author }) => ({
        number,
        author,
      })),
      [{ number: 15, author: "community-member" }],
    );
    assert.deepEqual(
      output.selected
        .map(({ number }) => number)
        .sort((left, right) => left - right),
      [10, 14],
    );
    assert.deepEqual(
      output.excludedActivePullRequests.map(({ issueNumber, pullRequests }) => ({
        issueNumber,
        pullRequests: pullRequests.map(({ number }) => number),
      })),
      [
        { issueNumber: 11, pullRequests: [90, 95] },
        { issueNumber: 12, pullRequests: [92] },
        { issueNumber: 13, pullRequests: [93] },
      ],
    );
  } finally {
    rmSync(temporaryDirectory, { recursive: true, force: true });
  }
});

test("documents evidence comments and controlled issue closure", () => {
  const skill = readFileSync(
    fileURLToPath(new URL("../SKILL.md", import.meta.url)),
    "utf8",
  );

  assert.match(skill, /Only issues authored by the repository owner/i);
  assert.match(skill, /Always leave one evidence-backed verification comment/i);
  assert.match(skill, /three distinct evidence-backed reports/i);
  assert.match(skill, /gh issue close .*--reason completed/);
});

test("requires pr-visuals assessment and player-observable before-after proof", () => {
  const skill = readFileSync(
    fileURLToPath(new URL("../SKILL.md", import.meta.url)),
    "utf8",
  );

  assert.match(skill, /Run the `pr-visuals` skill for every proposed fix/i);
  assert.match(skill, /Cutscene timing\/sequence fixes are always player-observable/i);
  assert.match(skill, /same deterministic request sequence against the exact base and fix/i);
  assert.match(skill, /looping GIF, a still PNG, and a labeled before\/after comparison/i);
  assert.match(skill, /neither embedded visual evidence nor an approved rationale/i);
});
