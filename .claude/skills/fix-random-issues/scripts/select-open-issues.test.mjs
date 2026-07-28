import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const selector = fileURLToPath(
  new URL("./select-open-issues.mjs", import.meta.url),
);

function issue(number) {
  return {
    number,
    title: `Issue ${number}`,
    html_url: `https://github.com/owner/repo/issues/${number}`,
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

test("excludes issues addressed by an open pull request before sampling", () => {
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
    assert.equal(output.openIssues, 5);
    assert.equal(output.available, 2);
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
