---
title: "Check PR State"
description: "Review current pull request status and help resolve any blockers"
tags: ["pr", "ci", "review"]
risk_level: "Low"
---

Review the current state of open pull requests and help address any issues preventing them from being merged. Focus on:

1. **Feedback**: Review feedback and comments on PR issues

   - If there is open feedback (a comment on the PR) without a response, it should be acted on. Choose one piece of actionable feedback per session.
   - Always respond to the specific piece of feedback. If there is a code change required to address the feedback, please make that change and push up the change.
   - Consider if a change to a prompt is required in response to the PR comment. If so, make the prompt change in .shodan/prompts or in CLAUDE.md
   - If the feedback is clearly and unambiguously resolved, you may resolve the comment. Otherwise, leave it open for the submitter to respond to.

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
   - No merge conflicts
   - Proper review approvals
   - Documentation updated if needed

5. **Issue Resolution**: Help resolve common PR problems:
   - Fix minor formatting or linting issues
   - Update documentation to match code changes
   - Resolve simple merge conflicts
   - Add missing tests for new functionality

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
