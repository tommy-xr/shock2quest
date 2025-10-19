---
title: "Fix Todos"
description: "Find a todo and fix it"
tags: ["pr", "todo"]
risk_level: "Low"
---

Search the codebase for TODOs and fix the TODO:

1. Find a TODO that is easily and quickly actionable, and not currently being worked on in active PRs.
2. Implement a simple, scoped fix - adding a test to verify correctness where possible.
   a. Use red/green test-driven development when adding tests - verify the test is first 'red' and then 'green' once the todo is implemented.
   b. Tests should be kept simple; avoid mocking
   c. Tests should not be trivial. For example, a test that verifies a constant is not a useful test.
