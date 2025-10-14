---
title: "Review GitHub Issues"
description: "Analyze open GitHub issues and potentially address small, well-defined ones"
tags: ["issues", "bugfix", "github"]
risk_level: "Medium"
---

Review the open GitHub issues in the repository and identify opportunities to address them. Focus on:

1. **Issue Triage**: Review open issues for:
   - Clear reproduction steps
   - Proper labeling and categorization
   - Current relevance (close outdated issues)
   - Duplicate detection

2. **Small Bug Fixes**: Look for issues that can be addressed with minimal, safe changes:
   - Documentation fixes
   - Minor UI improvements
   - Build script improvements
   - Test additions

3. **Issue Enhancement**: Improve issue quality by:
   - Adding reproduction steps where missing
   - Providing additional context or technical details
   - Linking related issues or PRs
   - Updating issue status based on current codebase

**Guidelines:**
- Only address issues that are clearly defined and have obvious solutions
- Focus on low-risk improvements (documentation, tests, minor fixes)
- Do not attempt complex features or major architectural changes
- Always verify fixes work before considering the issue resolved

**Safety Notes:**
- Avoid making changes to core game logic or rendering systems
- Do not modify VR-specific code without thorough understanding
- Test any changes thoroughly before committing
- Only close issues when genuinely resolved