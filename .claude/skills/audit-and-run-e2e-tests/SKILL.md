---
name: audit-and-run-e2e-tests
description: Run a repository's complete end-to-end suite, diagnose failures, verify focused fixes, and assess test infrastructure performance and reliability. Use for an E2E audit or failure sweep; follow the user's requested PR scope.
---

# Audit and run E2E tests

Produce an evidence-backed result: what ran, what failed, what changed, which fixes passed, and what remains untested. An audit request alone does not authorize publishing PRs; do that when the user asks for PRs or otherwise authorizes publication.

## Establish a trustworthy baseline

- Read repository guidance and identify the actual suite entry point, source test manifest, build steps, data requirements, opt-in fixtures, and resource constraints. In shock2quest, read [the repository-specific notes](references/shock2quest.md).
- Record the base revision, dirty files, runtime/build identity, asset identity, command, concurrency, free disk space, and log path. Monitor disk headroom during long runs, especially on a shared development host. Keep the user's checkout intact; use isolated branches/worktrees for fixes.
- Clean generated tests before compiling. A compiler can emit JavaScript despite returning an error: wait for its final exit status and require success before launching tests. Compare source and compiled manifests after changing branches; stale compiled tests can silently run against unrelated source.
- Keep one immutable executable and compiled test snapshot for the full baseline. Do not rebuild a shared target or edit its test output during the run. Shared Cargo targets across worktrees can reuse stale dependency artifacts; verify that the candidate actually contains changed dependencies before pinning it.
- Run the complete requested suite with captured logs and its real exit status. Preserve a failing baseline rather than stopping after the first failure. If setup is invalid (wrong assets, obsolete generated files), correct it and clearly identify the replacement baseline.
- Enumerate skips and TODOs separately from passes. Missing private saves are missing coverage, not passing tests. Locate the requested fixtures or ask for their paths while continuing independent work. Do not turn an exact campaign-save regression into a fabricated fresh-mission scenario just to remove its skip.

For Node TAP logs, `scripts/summarize_tap.py LOG --output REPORT.json` extracts counts, failures, skips, TODOs, timings, and completion evidence. It is a report helper, not a substitute for capturing the process exit code.

## Follow-up audits and pending PRs

- Fetch the current base and inventory which audit PRs remain open before reusing old branches or test results. Pull the user's checkout when requested, preserving unrelated changes; otherwise prepare the latest base in an isolated checkout. Record the new baseline revision.
- When rebasing/restacking is authorized, keep independent fixes based on the requested trunk and preserve genuine dependency chains. Scope the operation to the audit's PRs unless the user requests broader work. Inspect range-diffs, resolve conflicts against current behavior, and run relevant checks before pushing with an explicit expected-old-head force-with-lease.
- Refresh a PR's state immediately before publishing a follow-up. Pushing new commits to an already merged PR's branch does not make them reviewable; create a focused follow-up PR instead. Do not merge PRs merely because rebasing or auditing was requested.
- Report the new baseline separately from prior rounds and from rebased-branch checks. A private fixture discovered after a full run starts can be verified in a separate run without altering the active snapshot.

## Diagnose and fix

Maintain a compact ledger mapping every failure to its test, reproduction, cause, branch/commit/PR, and verification. Group failures sharing a demonstrated cause; separate unrelated product and infrastructure changes.

Before changing an assertion, determine which layer failed:

- Environment or harness: data version, build identity, stale output, ownership, transport, timeout, or cleanup.
- Fixture: a prerequisite, valid placement, inventory capacity, live identity, current interaction, or observer survival is missing.
- Product: a valid player action or same-build save/load violates the intended behavior.
- Nondeterminism: the same valid scenario varies; identify the source before adding waits, retries, or broader tolerances.

Reproduce narrowly against the pinned baseline. When the run preserved a checkpoint or save immediately before failure, copy it into an isolated diagnostic environment and replay that stage before repeatedly rebuilding the entire scenario. Inspect actual state at the first divergence, including whether the player is alive, the intended object is held, the input was accepted, and the expected action occurred. Preserve the test's behavioral purpose. Assert prerequisites explicitly; do not disguise a product failure with larger timeouts, retries, weakened assertions, or a skip.

For a product fix, demonstrate failure before and success after when feasible. For fixture changes, establish the current contract from production code/data and retain the consequential gameplay assertions. Run the affected test and relevant neighbors; repeat only when timing, shared helpers, or another unresolved concern makes repetition useful.

When PRs are requested, publish one independently reviewable logical fix per PR, using repository conventions. Explain the trigger, resulting behavior, root cause, and actual validation. Assess visual-evidence requirements for product changes; a test-only assertion update does not itself change rendered output.

## Combined verification and infrastructure review

- Combine verified commits in a separate verification branch, resolve conflicts, and perform a clean compile. Check asynchronous tool sessions to completion; a launched check is not a passed check.
- Pin the combined runtime and run the entire requested suite again. Freeze its compiled output until it finishes. Record any fixes that miss that snapshot and validate them separately: affected tests and relevant neighbors for a local fixture fix, broader verification for shared behavior. Report the full-run counts and follow-up reruns distinctly; never rewrite the original failure count as a pass.
- If a host interruption prevents completion, preserve the partial log and actual exit evidence. To resume an unchanged snapshot, record the completed file prefix and rerun the entire interrupted file plus every remaining file. Report verification as multiple segments, exclude partial-file results from aggregate counts, and keep the original interruption visible. Do not infer completion from a passing resumed subset.
- Keep own runtimes and test workers accounted for. Shutdown normal runs. On interruption, verify that the wrapper, runner, workers, and runtime descendants exited; do not stop unrelated sessions. Detached runtimes need their own cleanup path. Allow worker cleanup hooks to finish before escalating, even if the parent runner exits first.
- Assess performance with evidence: build count, startup/load costs, simulation-heavy cases, failure-diagnosis time, and resource contention. Separate measured improvements from proposals. Compare like-for-like runs and disclose cache/concurrency differences.
- Check CI coverage as well as local commands. Compile fixtures and run asset-independent harness tests in CI; reserve expensive platform builds for changes that need them. Before changing workflow path filters, verify required-check behavior and retain coverage for mixed or shared-file changes. Distinguish baseline/toolchain failures from regressions introduced by a fix.
- Inspect reliability mechanisms that can make a green result misleading: stale artifacts, zero-test filters/repetition counts, swallowed exit codes, missing fixtures, unbounded HTTP/body waits, shared mutable saves/settings, and orphaned processes. Add focused negative-first regression coverage for meaningful harness fixes.

Finish with baseline and verification counts, links to the fixes, remaining skips/TODOs, verified infrastructure changes, and prioritized future improvements. Keep logs and the ledger available for review. Do not claim the whole suite is green when private-fixture tests remain unrun.
