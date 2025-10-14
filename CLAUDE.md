# Claude Development Guidelines

## Core Principles

### 1. Small, Incremental Changes

- **Always break larger changes into smaller, manageable steps**
- Make one logical change per commit
- Test each increment before moving to the next
- Prefer multiple small PRs over large, complex ones
- Each change should be independently reviewable and rollback-able

### 2. Development Workflow

- Read and understand existing code before making changes
- Follow existing patterns and conventions in the codebase
- Run tests after each change (if available)
- Use descriptive commit messages that explain the "why"

## Project Documentation

### Essential Reading

Before making any changes, review these documents:

- **`DEVELOPMENT.md`** - Setup and build instructions
- **`README.md`** - Project overview and goals
- All documents in `.notes`

### Reference Materials

- **`references/`** - Technical specifications and data formats
  - `cutscene_formats.md` - Cutscene format documentation
  - Various `.spew` files with animation and sound data

## Project Structure

This is a Rust-based VR port of System Shock 2. Key components:

- `dark/` - Dark engine file format readers (bin, mis, cal, gam, etc)
- `engine/` - Core OpenGL rendering engine
- `shock2vr/` - Core gameplay logic
  - `scripts/` - Object script implementations
  - `mission/` - Mission running logic
  - `save_load/` - Game state serialization
  - `creature/` - Creature definitions and hitboxes
- `runtimes/` - Platform-specific runtime implementations
  - `desktop_runtime/` - Desktop version
  - `oculus_runtime/` - Oculus Quest VR version
  - `tool/` - Development tools for viewing models

## Development Commands

### Building

- Desktop: `cd runtimes/desktop_runtime && cargo run --release`
- Quest VR: `cd runtimes/oculus_runtime && source ./set_up_android_sdk.sh && cargo apk run --release`

### Code Quality

- Check code: `cargo check`
- Format code: `cargo fmt`
- Lint code: `cargo clippy`
- Run tests: `cargo test`

## Incremental Change Process

### 1. Research Phase

- Read relevant source files and understand data flow
- Check `docs/` folder for architectural context
- Review `references/` for technical specifications
- Understand existing patterns and conventions

### 2. Planning Phase

- Break the change into 2-3 small, logical steps maximum
- Identify which files need modification
- Plan testing approach for each increment
- Consider VR performance implications:
  - Quest hardware constraints require efficient code
  - Analyze rendering paths for frame rate optimization
  - Review memory usage patterns

### 3. Implementation Phase

- Make minimal changes to achieve one specific goal
- If there are issues found, like a bug or potential refactoring, that are outside of the scope of the current goal, you _MAY_ open an issue with enough details to make it actionable in a separate pass.
- Follow existing code patterns and naming conventions
- Test on desktop runtime first
- Validate each step before proceeding

### 4. Validation Phase

- Run `cargo check` and `cargo clippy`
- Test core functionality on desktop
- Verify VR compatibility if changes affect rendering
- Update documentation in `docs/` if architectural changes were made

## Common Change Categories

### Small Changes (Single commit)

- Bug fixes in specific functions
- Adding new configuration options
- Updating existing UI elements
- Performance optimizations in isolated code

### Medium Changes (2-3 commits)

- New gameplay features
- Refactoring a single module
- Adding new file format support
- UI/UX improvements

### Large Changes (Multiple small PRs)

- New major systems (break into multiple features)
- Architectural refactoring (one module at a time)
- Cross-platform compatibility changes
- Major performance overhauls

## Notes and Documentation

- Update `docs/` folder when making architectural decisions
- Document complex VR interactions and performance considerations
- Keep `CLAUDE.md` updated with new workflow discoveries
- Add new reference materials to appropriate folders

**Remember: Small, frequent, well-tested changes are always preferred over large, complex modifications.**

## Getting Help

- Check existing code for similar patterns
- Follow the incremental development process outlined above
- Focus on code analysis tasks:
  - Performance bottleneck identification
  - Code pattern consistency
  - Memory usage optimization
  - Rendering pipeline efficiency
- Use desktop runtime for rapid iteration and debugging
- It may be necessary to create one-off CLI tools to exercise functionality - feel free to add these as part of the PR. This is especially useful when a change may require understanding the games metadata (ie, the .gam or .mis files) - using our existing parsing tools to query the data can help with understanding the format.

## Testing

- Make sure, when adding a test that exercises code in a PR, to do a _negative_ test first - it should fail without the necessary change. Then, validate the code change makes it green
