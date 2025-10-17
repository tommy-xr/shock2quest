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

## Entity System Workflow

The System Shock 2 entity system is central to game logic. Understanding it is crucial for most gameplay modifications.

### Core Entity Concepts

- **Templates**: Blueprint definitions with unique IDs (stored in gamesys + mission files)
- **Properties**: Data components defining entity behavior (P$ chunks)
- **Links**: Relationships between entities (L$ chunks, with optional LD$ data)
- **Inheritance**: MetaProp links create template hierarchies
- **Scripts**: Rust implementations providing entity logic

### Key Files and Data Flow

```
Data Files:
├── shock2.gam (gamesys)     - Base templates, common objects
└── *.mis (missions)         - Level-specific entities, overrides

Parsing:
├── dark/src/gamesys/        - Gamesys parsing (shock2.gam)
├── dark/src/ss2_entity_info.rs - Core entity data structures
├── dark/src/properties/     - Property definitions (P$ chunks)
└── dark/src/mission/        - Mission file parsing (*.mis)

Runtime:
├── shock2vr/src/mission/mod.rs - Entity merging and instantiation
├── shock2vr/src/mission/entity_creator.rs - Entity creation logic
└── shock2vr/src/scripts/    - Entity behavior implementations
```

### Common Entity Tasks

#### Debugging Entity Issues

1. **Find entity by name**:

```bash
# Use grep to find entities with specific names in .gam/.mis files
rg "EntityName" Data/
```

2. **Trace entity inheritance**:

```rust
// In your debugging code
let ancestors = ss2_entity_info::get_ancestors(hierarchy, &template_id);
println!("Template {} inherits from: {:?}", template_id, ancestors);
```

3. **List entity properties**:

```rust
// Check what properties an entity has
for (template_id, props) in &entity_info.entity_to_properties {
    println!("Template {}: {} properties", template_id, props.len());
}
```

#### Working with Properties

1. **Add new property type**:

   - Define in `dark/src/properties/mod.rs`
   - Add parsing logic following existing patterns
   - Update property registration in `get()` function

2. **Debug property inheritance**:

   - Properties are resolved during entity creation
   - Child properties override parent properties
   - Some properties (Scripts) use merge logic instead

3. **Runtime property access**:

```rust
// Query entities by property
world.run(|v_model: View<PropModelName>| {
    for (entity_id, model) in v_model.iter().with_id() {
        println!("Entity {} uses model: {}", entity_id, model.0);
    }
});
```

#### Analyzing Links

1. **MetaProp links** (inheritance):

```rust
// These define template inheritance: child -> parent
Link { src: child_template, dest: parent_template, ... }
```

2. **Behavioral links** (Contains, Flinderize, etc.):

```rust
// Find what an entity contains
if let Ok(links) = v_links.get(entity_id) {
    for link in &links.to_links {
        match &link.link {
            Link::Contains(_) => println!("Contains entity {:?}", link.to_entity_id),
            Link::Flinderize(_) => println!("Will flinderize when destroyed"),
            _ => {}
        }
    }
}
```

### Entity System Debugging

#### Common Problems

1. **Missing entities**: Check gamesys merging and template ID resolution
2. **Wrong properties**: Verify inheritance chain and property override logic
3. **Broken scripts**: Ensure script files exist and are registered
4. **Physics issues**: Check PropPhysType, PropPhysDimensions, and collision setup

#### Debugging Commands

```rust
// Print all MetaProp links (inheritance relationships)
for link in &entity_info.link_metaprops {
    println!("MetaProp: {} inherits from {}", link.src, link.dest);
}

// Show entity creation process
let template_to_entity_id = entity_populator.populate(&entity_info, &level, &mut world);
for (template_id, entity_id) in &template_to_entity_id {
    println!("Created entity {} from template {}", entity_id.0, template_id);
}
```

#### Performance Considerations

- Entity inheritance is resolved at creation time, not runtime
- Properties are shared via `Rc<Box<dyn Property>>` for memory efficiency
- Typical missions have 1000-5000 entities
- MetaProp link traversal can be expensive for deep hierarchies

### File Format Investigation

When working with entity data, you may need to examine raw game files:

1. **Parse specific chunks**:

```bash
# Create small CLI tools to examine file structure
cargo run --bin tool -- inspect-gamesys shock2.gam
cargo run --bin tool -- list-entities medsci1.mis
```

2. **Compare gamesys vs mission data**:

   - Gamesys contains base templates and common definitions
   - Mission files override/extend gamesys data for level-specific needs
   - The `merge_with_gamesys()` function combines both sources

3. **Verify property data**:
   - Property chunks have 8-character names (P$Position, P$Scripts, etc.)
   - Each property has a length prefix and binary data
   - Property parsing must exactly match Dark Engine format

### Entity System Reference

See `references/entities.md` for comprehensive documentation of:

- Template inheritance mechanisms
- Property types and data formats
- Link types and their purposes
- File format specifications
- Code architecture details

## Development Commands

### Building

- Desktop: `cd runtimes/desktop_runtime && cargo run --release`
- Quest VR: `cd runtimes/oculus_runtime && source ./set_up_android_sdk.sh && cargo apk run --release`

### Experimental Features

The project supports experimental flags for gating in-progress features during development:

#### Using Experimental Flags

- Add `--experimental` flag followed by feature names when running desktop runtime
- Example: `cargo run -- --experimental teleport`
- Multiple features: `cargo run -- --experimental teleport,feature2`

#### Available Experimental Features

- **`teleport`**: VR teleport movement system
  - Enables point-and-teleport locomotion for VR comfort
  - Alternative to smooth movement that can cause motion sickness
  - Triggered via controller trigger button

#### Adding New Experimental Features

1. **Gate the feature in code**:

   ```rust
   if options.experimental_features.contains("feature_name") {
       // Enable feature logic
   }
   ```

2. **Initialize with conditional logic**:

   ```rust
   let feature_system = if options.experimental_features.contains("feature_name") {
       FeatureSystem::enabled()
   } else {
       FeatureSystem::disabled()
   };
   ```

3. **Update this documentation** to list the new experimental feature

This approach allows:

- Safe iteration on experimental features without affecting stable gameplay
- Easy enabling/disabling of features for testing
- Gradual rollout and user testing
- Clean separation between stable and experimental code paths

### Code Quality

- Check code: `cargo check`
- Format code: `cargo fmt`
- Lint code: `cargo clippy`
- Run tests: `cargo test`

### Build Validation

**MANDATORY: Both runtimes must compile before committing any changes.**

#### Desktop Runtime Validation

```bash
cd runtimes/desktop_runtime
cargo build
```

#### Oculus Runtime Validation

```bash
cd runtimes/oculus_runtime
# REQUIRED: Set up Android SDK environment
source ./set_up_android_sdk.sh
cargo apk check
```

**Note**: The oculus runtime requires Android SDK setup and will fail to compile on macOS/Linux without proper Android cross-compilation environment. Always run `source ./set_up_android_sdk.sh` first.

#### Build Validation Workflow

1. Make changes to core crates (`shock2vr`, `dark`, `engine`)
2. **Immediately** validate both runtimes compile:

   ```bash
   # Test desktop runtime
   cd runtimes/desktop_runtime && cargo build

   # Test oculus runtime
   cd ../oculus_runtime && source ./set_up_android_sdk.sh && cargo apk check
   ```

3. Fix any compilation errors before proceeding
4. Only commit when both runtimes compile successfully

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
- **CRITICAL: Run `cargo check` after each logical group of changes**
  - Especially important for trait/interface changes that affect multiple files
  - Validate compilation before moving to next step
  - See "Build Validation" section for mandatory runtime checks
- Test on desktop runtime first
- Validate each step before proceeding

### 4. Validation Phase

- **MANDATORY: Ensure code compiles before committing**
  - Run `cargo check` and `cargo clippy`
  - Fix all compilation errors - never commit broken code
  - For trait changes: verify ALL implementations are updated
- **CRITICAL: Run `cargo check` after each logical group of changes**
  - Especially important for trait/interface changes that affect multiple files
  - Validate compilation before moving to next step
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

## Special Considerations for Trait/Interface Changes

When modifying trait definitions or function signatures:

1. **Change the trait definition first**
2. **Immediately run `cargo check` to identify ALL affected implementations**
3. **Fix each implementation before proceeding**
4. **Re-run `cargo check` after each fix to ensure progress**
5. **Only commit when compilation is successful**

This pattern prevents leaving the codebase in a broken state and ensures all implementations stay in sync.

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
