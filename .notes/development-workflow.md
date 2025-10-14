# Development Workflow

## Incremental Development Guidelines

### Change Strategy

1. **Analyze First** - Understand existing code and patterns
2. **Plan Small** - Break large features into 2-3 file changes max
3. **Test Each Step** - Verify functionality after each increment
4. **Document Decisions** - Note complex logic and architectural choices

### Typical Change Process

1. **Research Phase**

   - Read relevant source files
   - Check references/ folder for technical specs
   - Understand data flow and dependencies

2. **Implementation Phase**

   - Make minimal changes to achieve one specific goal
   - Follow existing code patterns and naming conventions
   - Test on desktop runtime first, then VR if applicable

3. **Validation Phase**
   - Run `cargo check` and `cargo clippy`
   - Test core functionality
   - Verify VR compatibility if changes affect rendering

### Testing Strategy

- Desktop testing: `cd runtimes/desktop_runtime && cargo run --release`
- VR testing: Use Oculus runtime for final validation
- Unit tests: `cargo test` (when available)

### Common Pitfalls to Avoid

- Large, monolithic changes that touch many systems
- Breaking existing save/load compatibility
- Performance regressions in VR rendering
- Changes that don't follow Rust borrowing conventions

### Submitting PRs

- Keep PR descriptions short and concise - only focusing on the changes between the current branch and the parent branch.
