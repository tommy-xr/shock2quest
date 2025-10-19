# Shodan Automation Context

This session is running under Shodan automation with the following constraints:

## Safety Guidelines
- Only make incremental, safe improvements
- Do not modify core VR functionality without thorough understanding
- Focus on documentation, testing, and minor improvements
- Always test changes before committing
- Because this is automation, bias towards making decisions without user intervention.
- Keep changes as simple as possible.

## Project Context
- This is a VR port of System Shock 2 for Oculus Quest
- Written in Rust with OpenGL rendering
- Performance is critical for VR (90+ FPS)
- Follow existing code patterns and conventions

## Workflow
- Once you have decided on a work item, create a new branch with git
  - If there is pending work in a PR that you are working off of, use that latest branch
  - Otherwise, based your new branch on main
  - Use 'gt track' once the branch is created so graphite is aware of it
  - When the atom of work is complete:, make sure to update the issue, project description, docs, etc as well as part of the change.
  - make sure to update the issue, project description, docs, etc as well as part of the change.
  - Push a PR up with all of the changes - make sure the base is relative to the branch you worked off of
  - If you identify an issue or project that is outside the scope of the current work stream, avoid scope creep, but you may do one of the following:
      - Add a TODO item in the codebase (small tasks)
      - Open an issue against the codebase (medium task) - provide as much context as possible
      - Start a new file in projects to document the project (large task)

## Summarization & Continuous Improvement
Once the workstream is complete, append a journal entry to .notes/journal.md, containing:
- A single sentence describing the work done.
- A single sentence for continuous improvement - a piece of data that you learned that would've been useful, a suggestion for prompt improvement, or a tool that could've assisted.
