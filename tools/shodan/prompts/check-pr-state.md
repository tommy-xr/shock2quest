---
title: "Check PR State"
description: "Review current pull request status and help resolve any blockers"
tags: ["pr", "ci", "review"]
risk_level: "Low"
---

Review the current state of open pull requests and help address any issues preventing them from being merged. Focus on:

1. **CI/CD Status**: Check for failing builds or tests:
   - Analyze build logs for compilation errors
   - Review test failures and their causes
   - Check for linting or formatting issues
   - Verify that all required checks are passing

2. **PR Quality Review**: Ensure PRs meet quality standards:
   - Review commit messages for clarity
   - Check for proper documentation of changes
   - Verify that changes align with PR description
   - Ensure code follows project conventions

3. **Merge Readiness**: Assess whether PRs are ready to merge:
   - All CI checks passing
   - No merge conflicts
   - Proper review approvals
   - Documentation updated if needed

4. **Issue Resolution**: Help resolve common PR problems:
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