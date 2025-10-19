---
title: "Check PR State"
description: "Review current pull request status and help resolve any blockers"
tags: ["pr", "ci", "review"]
risk_level: "Low"
---

Review the current state of open pull requests and help address any issues preventing them from being merged.

## Branch Verification (Critical First Step)

Before making any changes to fix PR issues:

1. **Verify the PR branch**: Use `gh pr view <PR_NUMBER> --json headRefName` to confirm the exact branch name
2. **Check current branch**: Use `git branch --show-current` to verify you're on the correct branch
3. **Switch if needed**: Use `git checkout <correct-branch-name>` if there's a mismatch
4. **Confirm alignment**: Ensure your local branch matches the PR's head branch before proceeding

## Focus Areas

## Branch Verification (Critical First Step)

Before making any changes to fix PR issues:

1. **Verify the PR branch**: Use `gh pr view <PR_NUMBER> --json headRefName` to confirm the exact branch name
2. **Check current branch**: Use `git branch --show-current` to verify you're on the correct branch
3. **Switch if needed**: Use `git checkout <correct-branch-name>` if there's a mismatch
4. **Confirm alignment**: Ensure your local branch matches the PR's head branch before proceeding

## Focus Areas

1. **Feedback**: Review feedback and comments on PR issues

   - If there is open feedback (a comment on the PR) without a response, it should be acted on. Choose one piece of actionable feedback per session.
   - Always respond to the specific piece of feedback. If there is a code change required to address the feedback, please make that change and push up the change.
   - Consider if a change to a prompt is required in response to the PR comment. If so, make the prompt change in .shodan/prompts or in CLAUDE.md
   - If the feedback is clearly and unambiguously resolved, you may resolve the comment. Otherwise, leave it open for the submitter to respond to.

### Finding Actionable Feedback (Step-by-Step)

**Step 1: List all open PRs and identify those needing attention**
```bash
gh pr list --json number,title,headRefName,state,statusCheckRollup,mergeable,mergeStateStatus
```

**Step 2: For each PR, check for actionable comments using GitHub API**
```bash
# Get all review comments (code-level feedback)
gh api /repos/tommy-xr/shock2quest/pulls/<PR_NUMBER>/comments

# Get general PR comments (conversation-level feedback)
gh api /repos/tommy-xr/shock2quest/pulls/<PR_NUMBER>/issues/comments
```

**Step 3: Parse comments for actionable feedback**
Look for comments from repository owners or reviewers that:
- Ask questions requiring responses
- Request code changes or improvements
- Suggest alternative approaches
- Point out issues or bugs
- Request documentation updates

**Step 4: Prioritize feedback by type**
1. **Code change requests** (highest priority)
2. **Architecture/design feedback**
3. **Documentation requests**
4. **Style/convention suggestions**
5. **Clarification questions**

**Example: Identifying actionable feedback**
```bash
# This command revealed actionable feedback on PR #81:
gh api /repos/tommy-xr/shock2quest/pulls/81/comments
# Result: "How do we test this? Can we add a spotlight to the players hands, and gate with a 'enhanced_lighting' flag?"
```

**Response Strategy:**
- **Address the specific request** (add code, update docs, answer question)
- **Explain scope** if request requires major changes outside PR scope
- **Propose alternatives** if direct implementation isn't feasible
- **Commit and push changes** when code modifications are made

### Real Example Workflow

**Scenario**: Found 3 PRs with actionable feedback that were initially missed

**PR #78 - Template ID fix**:
```bash
gh api /repos/tommy-xr/shock2quest/pulls/78/comments
# Found: "This test is overkill, remove it" + "Use Option<i32> instead?"
gh pr view 78 --json headRefName  # Get: fix/null-template-id-todo
git checkout fix/null-template-id-todo
# Made changes: removed test, explained Option scope in comment
git commit && git push
gh pr comment 78 --body "✅ Addressed both feedback items..."
```

**PR #79 - Asset validation**:
```bash
gh api /repos/tommy-xr/shock2quest/pulls/79/comments
# Found: "Use existing GUI system in gui_component.rs"
# Analysis: Requires significant refactor (~50+ lines)
gh pr comment 79 --body "✅ You're right, analyzed scope: substantial refactor required..."
# Proposed follow-up PR approach for incremental changes
```

**Key Lessons**:
- **API approach catches hidden feedback** that simple PR views miss
- **Branch verification prevents wasted work** on wrong branches
- **Scope assessment** helps decide immediate fix vs follow-up PR
- **Clear communication** explains decisions and next steps

2. **CI/CD Status**: Check for failing builds or tests:

   - Analyze build logs for compilation errors
   - Review test failures and their causes
   - Check for linting or formatting issues
   - Verify that all required checks are passing

3. **PR Quality Review**: Ensure PRs meet quality standards:

   - Review commit messages for clarity
   - Check for proper documentation of changes
   - Verify that changes align with PR description
   - Ensure code follows project conventions

4. **Merge Readiness**: Assess whether PRs are ready to merge:

   - All CI checks passing
   - No merge conflicts (check `mergeable` and `mergeStateStatus` fields)
   - Proper review approvals
   - Documentation updated if needed

### Merge Conflict Detection and Resolution

**Critical Check**: Always verify merge status when listing PRs:
```bash
gh pr list --json number,title,mergeable,mergeStateStatus
```

**Merge Status Values:**
- `"mergeable": "MERGEABLE"` + `"mergeStateStatus": "CLEAN"` = Ready to merge
- `"mergeable": "CONFLICTING"` + `"mergeStateStatus": "DIRTY"` = **Has merge conflicts**
- `"mergeable": "UNKNOWN"` = GitHub still calculating, check again

**When Conflicts Found:**
1. **Switch to PR branch**: `git checkout <branch-name>`
2. **Update from remote**: `git pull origin <branch-name>`
3. **Merge base branch**: `git merge origin/main` (or appropriate base)
4. **Resolve conflicts**: Edit conflicted files, remove conflict markers
5. **Test build**: Ensure `cargo build` still works after resolution
6. **Commit resolution**: `git add . && git commit -m "resolve merge conflicts"`
7. **Push resolution**: `git push`

5. **Issue Resolution**: Help resolve common PR problems:
   - Fix minor formatting or linting issues
   - Update documentation to match code changes
   - **Resolve merge conflicts** (critical blocker)
   - Add missing tests for new functionality

### Priority Order for PR Issues:
1. **Merge conflicts** (blocks all merging)
2. **CI build failures** (prevents validation)
3. **Actionable code review feedback** (blocks approval)
4. **Documentation/style issues** (quality improvements)

## After Making Changes

- **Verify commits appear in PR**: Use `gh pr view <PR_NUMBER> --json commits` to confirm your changes are reflected
- **Check CI triggers**: Ensure new CI runs are triggered with your latest commit hash
- **Monitor build status**: Watch for updated status checks after pushing changes

**Guidelines:**

- Only make minimal changes necessary to get PRs passing
- Focus on fixing CI issues rather than changing functionality
- Do not make major modifications to PR content
- Respect the original author's intent and implementation choices

**Safety Notes:**

- Do not modify core functionality within existing PRs
- Only fix obvious errors or missing pieces
- Avoid changing the scope or goals of existing PRs
- Always verify changes don't break existing functionality
