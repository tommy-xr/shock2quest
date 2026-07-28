---
name: fix-random-issues
description: >-
  Randomly sample X open GitHub issues from shock2quest and process them
  sequentially, delegating one issue at a time to an isolated agent that must
  reproduce or confirm the problem, implement a focused fix, verify it, and open
  a reviewable PR. Use for requests to fix, sweep, tackle, or work through a
  bounded number of random open shock2quest issues.
---

# Fix random issues

Run a bounded issue-fixing sweep. Freeze one unbiased random sample, then give
each selected issue to exactly one issue worker. Finish or stop that worker
before starting the next.

## Inputs

- Read `X` as the maximum number of sampled issues; default to `1`. Require a
  positive integer.
- Resolve the repository from the checkout, normally
  `tommy-xr/shock2quest`. Honor an explicit repository override.
- Accept an optional seed. Generate and report one when omitted.

Do not silently replace skipped, closed, duplicate, already-fixed, or
unreproducible selections. Each still consumes one sample slot. This preserves
the random sample and makes the run produce up to X fixes rather than
cherry-picking X easy fixes. If fewer than X issues are open, process all of
them and report the smaller sample.

## 1. Preflight and freeze the sample

1. Read `AGENTS.md`, `README.md`, and `DEVELOPMENT.md`.
2. Confirm `gh` authentication, repository access, and push/PR access before
   starting a worker. Never merge a PR or close an issue directly.
3. Record the checkout's initial status. Preserve all existing user changes.
   Use an isolated branch/worktree for every issue; never stack unrelated issue
   fixes.
4. Select the full sample once:

   ```bash
   node .agents/skills/fix-random-issues/scripts/select-open-issues.mjs X \
     --repo tommy-xr/shock2quest
   ```

   Add `--seed <value>` when supplied. `.agents/skills` is the compatibility
   symlink to `.claude/skills`, so this command works for both Claude and Codex.
5. Keep the emitted repository, seed, and ordered `selected` array as the run
   ledger. Do not rerun the draw unless the user explicitly requests a new
   sample.

The selector samples all open issues without inspecting difficulty, labels, or
assignees first. Random means random; do not quietly filter the pool.

## 2. Process the selected issues serially

For each selected issue, in emitted order:

1. Re-read it with `gh issue view`, including its current state, body, comments,
   labels, and assignees. Skip it if it is no longer open. Check current
   `origin/main` for an existing fix or superseding PR before editing.
2. Start one issue worker using the host's delegation tool. Prefer a
   host-provided isolated worktree. Otherwise create a unique branch/worktree
   from current `origin/main`, named along the lines of
   `fix/issue-<number>-<slug>`.
3. Give the worker the prompt contract below and the worktree path. Wait for its
   final result. Do not start another issue worker concurrently. If the worker
   uses a required helper such as `pr-visuals`, wait for that helper too.
4. Inspect the worker's evidence, diff, tests, commit, PR, and CI result. Ask the
   same worker for corrections when its result is incomplete. Do not take over
   silently or delegate the issue to a second worker.
5. Record one terminal result:
   - `fixed`: reproduction/confirmation, focused fix, local verification,
     conventional commit, PR containing `Fixes #N`, and green CI.
   - `skipped`: closed, duplicate, superseded, or already fixed on current main.
   - `not reproduced`: reasonable evidence failed to confirm the report.
   - `blocked`: a concrete product decision, unavailable dependency/hardware,
     unsafe scope, or persistent verification/CI failure prevents completion.
6. Remove a temporary worktree only when it is clean and every change is safely
   committed and pushed. Otherwise preserve it and report its path.

One issue is complete before the next begins. Sequential processing is a
correctness boundary, not merely a preference.

## Issue-worker prompt contract

Pass the issue number, URL, frozen title, repository, base branch, and isolated
worktree. Instruct the worker:

```text
Own issue #N end to end in the supplied isolated worktree.

Read AGENTS.md, README.md, and DEVELOPMENT.md first. Read the full issue and
comments. Work only on this issue; preserve unrelated changes.

REPRODUCE: Confirm the reported behavior on current origin/main before changing
code. For a feature gap, confirm the missing/stubbed behavior and investigate
the faithful System Shock 2 flow. Add and run a negative test first whenever a
test can exercise the fix. Do not guess or implement a speculative fix when the
issue cannot be reproduced.

FIX: Make the smallest faithful change that resolves the issue through the
game's real systems. Follow existing patterns. Do not use debug-only shims as
the product fix. Keep one logical conventional commit.

VERIFY: Run the focused test, the repository's warning-free package checks,
format checks, and every runtime/SDK/e2e verification required by AGENTS.md for
the affected area. For visual changes, use the pr-visuals skill and include the
required still/GIF and before/after evidence.

DELIVER: Fetch and rebase onto current origin/main, rerun affected verification,
push the branch, and open one conventional-title PR whose body includes
reproduction evidence, the fix, tests, and `Fixes #N`. Watch PR checks to a
terminal result and fix failures caused by the change. Never merge the PR or
close the issue yourself.

Return: status; pre-fix reproduction; root cause; files changed; tests and exact
results; commit SHA; PR URL; CI status; remaining risks. If skipped,
unreproduced, or blocked, make no speculative changes and return concrete
evidence plus the clean worktree status.
```

Treat a worker report without pre-fix evidence or without targeted verification
as incomplete. A PR URL alone is not completion.

## 3. Report the sweep

Return the repository, seed, requested count, actual sampled count, and a table
in sampled order with:

| Issue | Result | Evidence | Commit / PR | Verification |
|---|---|---|---|---|

State `fixed K of S sampled issues (X requested)`. List preserved worktree paths
and blockers. Do not claim skipped or merely patched issues as fixed.
