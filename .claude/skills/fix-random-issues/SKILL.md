---
name: fix-random-issues
description: >-
  Randomly sample X eligible owner-authored open GitHub issues from shock2quest,
  excluding issues already addressed by an open PR, and process them
  sequentially. Delegate one issue at a time to an isolated agent that must
  reproduce or confirm the problem, implement a focused fix, verify it, open a
  reviewable PR, or document and close a verified stale issue. Use for requests
  to fix, sweep, tackle, or work through a bounded number of random open
  shock2quest issues.
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
- Only issues authored by the repository owner are eligible. Exclude issues
  opened by every other user, collaborator, organization member, or bot before
  sampling.
- Accept an optional seed. Generate and report one when omitted.

Before drawing, exclude an issue when an open PR in the repository declares
that it closes, fixes, or resolves that issue. These issues are not part of the
eligible pool and do not consume a sample slot. This check deliberately reads
the open PR bodies as well as the open issue list, so stacked PRs are recognized
even when GitHub does not populate their default-branch closing metadata.

Do not silently replace skipped, closed, duplicate, already-fixed, or
unreproducible selections. Each still consumes one sample slot. This preserves
the random sample and makes the run produce up to X fixes rather than
cherry-picking X easy fixes. If fewer than X eligible issues remain, process all
of them and report the smaller sample. If a PR begins addressing a selected
issue after the sample is frozen, skip it without drawing a replacement.

## 1. Preflight and freeze the sample

1. Read `AGENTS.md`, `README.md`, and `DEVELOPMENT.md`.
2. Confirm `gh` authentication, repository access, and push/PR access before
   starting a worker. Never merge a PR. Comment on and close issues only under
   the verified-fix protocol below.
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
5. Keep the emitted repository, seed, `totalOpenIssues`, owner-authored
   `openIssues`, `excludedNonOwnerIssues`, `excludedActivePullRequests`, and
   ordered `selected` array as the run ledger. Do not rerun the draw unless the
   user explicitly requests a new sample.

The selector samples all eligible owner-authored open issues without inspecting
difficulty, labels, or assignees first. Author and active-PR exclusions are the
only pre-sampling filters. Random means random; do not quietly filter the
eligible pool further.

## 2. Process the selected issues serially

For each selected issue, in emitted order:

1. Re-read it with `gh issue view`, including its author, current state, body,
   comments, labels, and assignees. Stop and report a selector defect if the
   author is not the repository owner; do not draw a replacement. Skip it if it
   is no longer open or an open PR now declares that it closes, fixes, or
   resolves the issue. Check current `origin/main` for an existing fix or
   superseding merged PR before editing.
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
   - `skipped`: closed, duplicate, actively addressed by an open PR, superseded,
     or already fixed on current main. Apply the verified-fix protocol when the
     issue is still open.
   - `not reproduced`: reasonable evidence failed to confirm the report.
   - `blocked`: a concrete product decision, unavailable dependency/hardware,
     unsafe scope, or persistent verification/CI failure prevents completion.
6. Resolve any open, already-fixed issue under the verified-fix protocol below.
7. Remove a temporary worktree only when it is clean and every change is safely
   committed and pushed. Otherwise preserve it and report its path.

One issue is complete before the next begins. Sequential processing is a
correctness boundary, not merely a preference.

### Verified-fix protocol

Use this protocol only after the selected issue's worker returns evidence about
current `origin/main`. The manager owns comments and closure; the worker must
not close the issue.

1. Always leave one evidence-backed verification comment when an open selected
   issue appears fixed. Include the current main SHA, the fixing commit or
   merged PR when known, exact targeted verification and result, any remaining
   scope, and this hidden marker:

   ```html
   <!-- fix-random-issues: appears-fixed-report -->
   ```

   Read existing comments first. Do not post or count a duplicate of the same
   run or materially identical evidence.
2. Treat the fix as conclusive only when:
   - the issue's reported behavior or acceptance criteria pass a targeted test
     or live reproduction on current main;
   - the responsible merged change is present on current main when one exists;
   - no acceptance criterion remains unresolved; and
   - any adjacent product decision is explicitly out of scope or tracked
     separately.

   After posting the evidence comment, close a conclusive issue with
   `gh issue close N --repo OWNER/REPO --reason completed`.
3. When the issue merely appears fixed but the evidence is not conclusive, keep
   it open. Count only three distinct evidence-backed reports from separate
   verification runs, each carrying the marker above and identifying its tested
   main SHA. On the third qualifying report, add a closing comment that links
   the three reports and close the issue as completed.
4. Never close based only on failure to reproduce, passing CI, stale evidence,
   duplicated evidence, or an unverified code inspection. Report the remaining
   uncertainty instead.

## Issue-worker prompt contract

Pass the issue number, URL, frozen title, repository, base branch, and isolated
worktree. Instruct the worker:

```text
Own issue #N end to end in the supplied isolated worktree.

Read AGENTS.md, README.md, and DEVELOPMENT.md first. Read the full issue and
comments. Confirm the issue author is the repository owner. Work only on this
issue; preserve unrelated changes.

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
close the issue yourself. If current main already fixes the issue, return the
evidence and a `conclusive`, `appears fixed`, or `not fixed` closure
recommendation to the manager.

Return: status; pre-fix reproduction; root cause; files changed; tests and exact
results; commit SHA; PR URL; CI status; remaining risks. If skipped,
unreproduced, or blocked, make no speculative changes and return concrete
evidence plus the clean worktree status.
```

Treat a worker report without pre-fix evidence or without targeted verification
as incomplete. A PR URL alone is not completion.

## 3. Report the sweep

Return the repository and owner, seed, requested count, total open issue count,
owner-authored open issue count, eligible count, actual sampled count,
non-owner exclusion count, and the issues excluded because of active PRs. Then
provide a table in sampled order with:

| Issue | Result | Evidence | Issue action | Commit / PR | Verification |
|---|---|---|---|---|---|

State `fixed K of S sampled issues (X requested)`. List preserved worktree paths
and blockers. Report verification-comment and closure URLs, plus a count of
stale fixed issues closed. Do not claim skipped, closed-as-stale, or merely
patched issues as fixed.
