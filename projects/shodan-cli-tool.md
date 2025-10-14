# Shodan CLI Tool

Shodan is a Claude Code orchestrator that runs periodically (at most once an hour) to automatically work on project tasks using randomly selected prompts.

## Overview

Shodan monitors the repository state and, when no active Claude Code sessions are running, automatically:
1. Gets the latest code state
2. Selects a random prompt from available prompts
3. Runs Claude Code with the prompt
4. Monitors the resulting PR until it's green
5. Repeats the cycle

## Implementation Plan

### Phase 1: Core Infrastructure (1-2 days)

#### Task 1.1: Project Setup
- [ ] Create `tools/shodan/` directory structure
- [ ] Add `shodan` to workspace members in root `Cargo.toml`
- [ ] Create `tools/shodan/Cargo.toml` with dependencies:
  - `clap` for CLI parsing
  - `serde` + `serde_json` for JSON handling
  - `tokio` for async operations
  - `anyhow` for error handling
  - `chrono` for time management
  - `rand` for prompt selection
  - `reqwest` for HTTP calls (GitHub API)

#### Task 1.2: CLI Structure
- [ ] Create `tools/shodan/src/main.rs` with basic CLI commands:
  - `run` - Start the orchestration loop
  - `check` - Check current repository state
  - `test-prompt <file>` - Test a single prompt
- [ ] Implement configuration struct for settings
- [ ] Add logging setup using `tracing`

### Phase 2: Git Operations (1 day)

#### Task 2.1: Git Integration
- [ ] Create `git.rs` module with functions:
  - `get_current_branch()`
  - `checkout_main()`
  - `run_gt_sync()` (shell command execution)
  - `get_open_prs()` (via `gh` CLI)
  - `check_pr_status(pr_number)`

#### Task 2.2: Repository State Management
- [ ] Function to detect active Claude Code sessions
- [ ] Function to check for uncommitted changes
- [ ] Function to ensure clean working directory

### Phase 3: Prompt Management (1 day)

#### Task 3.1: Prompt System
- [ ] Create `prompts.rs` module
- [ ] Implement prompt discovery in `tools/shodan/prompts/`
- [ ] Random prompt selection with weighted preferences
- [ ] Prompt validation and formatting

#### Task 3.2: Initial Prompts
- [ ] Create `tools/shodan/prompts/iterate-on-projects.md`
- [ ] Create `tools/shodan/prompts/iterate-on-issues.md`
- [ ] Create `tools/shodan/prompts/check-pr-state.md`
- [ ] Create `tools/shodan/prompts/improve-documentation.md`
- [ ] Create `tools/shodan/prompts/optimize-performance.md`

### Phase 4: Claude Code Integration (2 days)

#### Task 4.1: Claude Code Orchestration
- [ ] Create `claude_code.rs` module
- [ ] Implement Claude Code subprocess management
- [ ] JSON input/output handling for `--input-format=json` and `--output-format=json`
- [ ] Session state tracking

#### Task 4.2: Claude Code Communication
- [ ] Parse Claude Code JSON responses
- [ ] Handle Claude Code errors and retries
- [ ] Implement timeout handling for long-running sessions

### Phase 5: PR Monitoring (1-2 days)

#### Task 5.1: GitHub Integration
- [ ] Create `github.rs` module using GitHub CLI (`gh`)
- [ ] Functions to:
  - Check PR status (draft, open, merged, closed)
  - Get CI/CD status (GitHub Actions, etc.)
  - Retrieve PR details and metadata
  - Get failing test logs

#### Task 5.2: CI Status Monitoring
- [ ] Parse CI failure logs
- [ ] Format failure information for Claude Code input
- [ ] Implement retry logic for CI monitoring

### Phase 6: Main Orchestration Loop (1 day)

#### Task 6.1: Scheduling and State Management
- [ ] Create `orchestrator.rs` module
- [ ] Implement hourly scheduling logic
- [ ] State persistence between runs
- [ ] Loop termination conditions

#### Task 6.2: Main Execution Flow
- [ ] Implement the complete orchestration cycle:
  1. Check if Claude Code is active
  2. Ensure clean git state
  3. Select and execute prompt
  4. Monitor PR creation
  5. Wait for green CI
  6. Schedule next iteration

### Phase 7: Error Handling & Polish (1 day)

#### Task 7.1: Robust Error Handling
- [ ] Comprehensive error types and handling
- [ ] Graceful degradation for network issues
- [ ] Recovery strategies for common failure modes
- [ ] Detailed logging and debugging information

#### Task 7.2: Configuration and Customization
- [ ] Configuration file support (`shodan.toml`)
- [ ] Environment variable configuration
- [ ] Adjustable scheduling intervals
- [ ] Prompt weight customization

## Project Structure

```
tools/
└── shodan/
    ├── Cargo.toml
    ├── src/
    │   ├── main.rs          # CLI entry point
    │   ├── config.rs        # Configuration management
    │   ├── git.rs           # Git operations
    │   ├── prompts.rs       # Prompt management
    │   ├── claude_code.rs   # Claude Code integration
    │   ├── github.rs        # GitHub/PR operations
    │   ├── orchestrator.rs  # Main orchestration logic
    │   └── lib.rs           # Library exports
    ├── prompts/
    │   ├── iterate-on-projects.md
    │   ├── iterate-on-issues.md
    │   ├── check-pr-state.md
    │   ├── improve-documentation.md
    │   └── optimize-performance.md
    └── shodan.toml          # Default configuration
```

## Usage Examples

```bash
# Start the orchestration loop
cargo run -p shodan run

# Check current state without running
cargo run -p shodan check

# Test a specific prompt
cargo run -p shodan test-prompt prompts/iterate-on-projects.md

# Run with custom config
cargo run -p shodan run --config custom-shodan.toml

# Run with different interval
cargo run -p shodan run --interval 30m
```

## Configuration Options

```toml
[shodan]
# Scheduling
interval = "1h"              # How often to run
max_session_time = "4h"      # Max time for Claude Code session

# Git settings
main_branch = "main"
sync_command = "gt sync"

# GitHub settings
check_interval = "5m"        # How often to check PR status
max_ci_wait_time = "30m"     # Max time to wait for CI

# Prompt settings
prompt_dir = "prompts"
[shodan.prompt_weights]
"iterate-on-projects.md" = 3
"iterate-on-issues.md" = 2
"check-pr-state.md" = 1
```

## Dependencies

Key Rust crates needed:
- `clap` (4.0+) - CLI argument parsing
- `tokio` (1.0+) - Async runtime
- `serde` + `serde_json` - JSON serialization
- `anyhow` - Error handling
- `chrono` - Date/time handling
- `rand` - Random selection
- `reqwest` - HTTP client (if needed for GitHub API)
- `tracing` + `tracing-subscriber` - Logging
- `toml` - Configuration parsing

## Success Criteria

1. **Automated Operation**: Shodan runs continuously and autonomously
2. **Safe Git Operations**: Never corrupts git state or interferes with manual work
3. **Robust Error Handling**: Gracefully handles network issues, CI failures, and Claude Code errors
4. **Configurable**: Easy to adjust timing, prompts, and behavior
5. **Observable**: Clear logging and status reporting
6. **Integration**: Works seamlessly with existing project workflow

## Future Enhancements

- Web dashboard for monitoring Shodan activity
- Slack/Discord notifications for completed tasks
- Machine learning for prompt selection optimization
- Integration with project management tools
- Support for multiple repositories
