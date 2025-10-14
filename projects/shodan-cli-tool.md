## Shodan

Shodan is a CLI tool that runs Claude Code periodically - at most once an hour - and picks a prompt randomly from the set of available prompts.

Essentially, Shodan is a Claude Code orchestrator, with the following responsibilities:
Every hour, assuming a claude code session is not active

- Get a list of current PRs
- `git checkout main`
- Run `gt sync` to get to the latest good state
- Pick a random prompt from the available prompts
- Run claude code with the prompt, using `--input-format=json` and `output-format=json`
- After each iteration, check if a new PR is created
- Wait for and check if the PR is green - if so, the task is complte, and we can wait until the next one
- If the PR is not green, identify the failing run and logs and provide it as input.
- Repeat until the PR is green

Can you help me create a rust CLI tool that does this?

Project structure:

- <root>
    - tools
        - shodan
            - prompts
                iterate-on-projects.md
                iterate-on-issues.md
                check-pr-state.md
            - cli

Ideally, we can run with something like cargo run -p shodan and let it keep building out the project.
