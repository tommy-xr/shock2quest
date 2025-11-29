# Claude Development Guidelines

For a concise contributor checklist, see `AGENTS.md`. This document serves as the extended reference for workflows and engine internals.

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
- `tools/` - Development CLI tools (dark_query, dark_viewer)

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

## Tooling Notes

### dark_query Speech Explorer

Use the `dark_query` CLI to inspect speech metadata without launching the game:

- List available voices and sample hints:

  ```bash
  cargo run -p dark_query -- speech
  ```

- Show all concepts and tags for a specific voice (index or alias like `voice6`):

  ```bash
  cargo run -p dark_query -- speech 2
  ```

- Query clips by tag filters (supports `+concept:name`, enum tags such as `+alertlevel:two`, and numeric ranges):

  ```bash
  cargo run -p dark_query -- speech 2 +concept:spotplayer +alertlevel:three
  ```

If no tags are supplied for a voice, the tool prints the concept list and per-tag metadata. When tags are provided, every matching schema and its samples (with frequency weights) are shown.

### File Format Investigation

When working with entity data, you may need to examine raw game files:

1. **Inspect model files**:

```bash
# Use dark_viewer to examine model structure
cargo dv grunt_p.bin

# Use dark_query to list entities
cargo dq entities medsci1.mis
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

### Cargo Aliases

For faster development, the project includes convenient cargo aliases (defined in `.cargo/config.toml`):

- `cargo dr` - Desktop runtime (shorthand for `cargo run -p desktop_runtime --`)
- `cargo dq` - Dark query CLI tool (shorthand for `cargo run -p dark_query --`)
- `cargo dv` - Dark viewer tool (shorthand for `cargo run -p dark_viewer --`)

Example usage:
```bash
cargo dr --release --experimental teleport
cargo dq entities earth.mis --filter "*Door*" --limit 10
cargo dv grunt_p.bin
```

**Note**: These aliases only work for desktop development. Android builds still require the full `cargo apk` commands.

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

**MANDATORY: Core crates must compile before committing any changes.**

#### Standard Validation

For most changes to core crates (`shock2vr`, `dark`, `engine`):

```bash
cargo check -p shock2vr
```

This validates the main game logic without requiring platform-specific setup.

#### Runtime-Specific Validation

Only validate specific runtimes when you've made changes to them:

```bash
# Desktop runtime (if changes made to runtimes/desktop_runtime)
cargo check -p desktop_runtime

# Oculus runtime (if changes made to runtimes/oculus_runtime)
cd runtimes/oculus_runtime
source ./set_up_android_sdk.sh
cargo apk check
```

**Note**: The oculus runtime requires Android SDK setup. Only validate it when making oculus-specific changes.

## Incremental Change Process

### 1. Research Phase

- Read relevant source files and understand data flow
- Check `references/` folder for technical specifications
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
- **CRITICAL: Run `cargo check -p shock2vr` after each logical group of changes**
  - Especially important for trait/interface changes that affect multiple files
  - Validate compilation before moving to next step
- Test on desktop runtime first
- Validate each step before proceeding

### 4. Validation Phase

- **MANDATORY: Ensure code compiles before committing**
  - Run `cargo check` and `cargo clippy`
  - Fix all compilation errors - never commit broken code
  - For trait changes: verify ALL implementations are updated
- Test core functionality on desktop
- Verify VR compatibility if changes affect rendering
- Update documentation if architectural changes were made

## Special Considerations for Trait/Interface Changes

When modifying trait definitions or function signatures:

1. **Change the trait definition first**
2. **Immediately run `cargo check` to identify ALL affected implementations**
3. **Fix each implementation before proceeding**
4. **Re-run `cargo check` after each fix to ensure progress**
5. **Only commit when compilation is successful**

This pattern prevents leaving the codebase in a broken state and ensures all implementations stay in sync.

## Entity Query CLI Tool (`dark_query`)

The `dark_query` CLI tool in `tools/dark_query` provides powerful analysis capabilities for debugging and understanding System Shock 2's entity system and motion database.

### Overview

```bash
# Run from project root directory
cargo dq --help  # Using cargo alias
# OR: cargo run -p dark_query -- --help

# Basic entity commands
cargo dq entities                    # List all entities and templates (gamesys only)
cargo dq entities earth.mis         # List entities with mission
cargo dq entities earth.mis 443     # Show detailed entity info

# Template commands (easier for negative template IDs)
cargo dq templates                   # List templates only (negative IDs)
cargo dq templates 22                # Show template -22 details (converts 22 to -22)
cargo dq templates 22 earth.mis     # Show template -22 with mission data

# Motion database commands
cargo dq motion 0                    # Show animations for ActorType::Human (0)
cargo dq motion human +playspecmotion +human  # Query specific tags
cargo dq motion 0 +cs:184            # Query with tag value
```

### Key Features

1. **Entity and Template Listing**:
   - Lists all templates (negative IDs) and entities (positive IDs)
   - **Templates command**: Use `cargo dq templates 22` instead of dealing with negative IDs like `-22`
   - Shows names, template IDs, property counts, link counts
   - Inheritance-aware name resolution (finds names from template hierarchy)
   - Supports filtering: `--filter "*Railing*"`, `--filter "P$SymName:*Robot*"`
   - Unparsed data detection: `--only-unparsed`

2. **Detailed Entity Analysis**:
   - Complete entity information with inheritance-aware names
   - **Bidirectional link analysis** with "Outgoing Links" and "Incoming Links" sections
   - **Inheritance tree visualization** from most specific to most general
   - Property details at each inheritance level
   - Demonstrates property inheritance and overrides

3. **Mission Support**:
   - Load entity data from gamesys only: `cargo run -p dark_query -- entities`
   - Load gamesys + mission: `cargo run -p dark_query -- entities earth.mis`
   - Automatically merges and resolves entity hierarchies

### Usage Examples

#### Basic Entity Exploration
```bash
# Find all entities with "Railing" in the name
cargo dq entities earth.mis --filter "*Railing*"

# Find entities with unparsed data (useful for development)
cargo dq entities --only-unparsed

# Show property value filtering
cargo dq entities --filter "P$SymName:*Robot*"

# Limit results for quick iteration and testing
cargo dq entities earth.mis --limit 5
cargo dq entities earth.mis --filter "*Door*" --limit 10
```

#### Detailed Entity Analysis
```bash
# Analyze entity 443 (Railing) inheritance and relationships
cargo dq entities earth.mis 443

# Analyze entity 442 (Tripwire) to see its switch links
cargo dq entities earth.mis 442

# Analyze template -1718 to understand railing template structure
cargo dq templates 1718 earth.mis   # Much easier than: cargo dq entities earth.mis -- -1718
```

#### Understanding Entity Relationships
```bash
# Entity 442 (Tripwire) shows:
# Outgoing Links:
#   1. SwitchLink -> Entity 445 (Sound Trap)
#   2. SwitchLink -> Entity 503 (Inverter)

# Entity 445 (Sound Trap) shows:
# Incoming Links:
#   1. Entity 442 (New Tripwire) -> SwitchLink here
```

#### Script Filtering Examples
```bash
# Search for entities with specific scripts using property value syntax (case-insensitive)
cargo dq entities earth.mis --filter "P$Scripts:stddoor"

# Search for entities with script names containing pattern (case-insensitive)
cargo dq entities earth.mis --filter "P$Scripts:*camera*"

# Alternative script search using S$ prefix (case-insensitive)
cargo dq entities earth.mis --filter "S$*stddoor*"

# Exact script name with S$ prefix (case-insensitive)
cargo dq entities earth.mis --filter "S$stddoor"

# General search across all entity data (properties, links, scripts)
cargo dq entities earth.mis --filter "*StdDoor*"

# Use --limit for quick iteration when testing script searches
cargo dq entities earth.mis --filter "S$stddoor" --limit 5
```

**Script Filtering Notes:**
- **Case-insensitive**: All script searches are now case-insensitive
- Use `P$Scripts:pattern` to search for entities with specific script names
- Use `S$pattern` as a shorthand for script-only searches
- Supports wildcards: `*pattern*` matches scripts containing "pattern"
- Scripts are inheritance-aware - child entities inherit parent scripts
- Matched items display shows full script names (e.g., `P$Scripts:StdDoor`, `S$cameradeath`)
- Improved display truncation shows up to 50 characters of matched items
- **Use `--limit N`** to show only first N results for quick iteration and testing

## Motion Database Queries

The `motion` command provides powerful querying capabilities for the System Shock 2 motion database, allowing you to explore creature animations and their tag-based organization.

### Motion Database Overview

```bash
# Basic motion queries
cargo dq motion 0                    # List available animations for Human
cargo dq motion human                # Same as above using name
cargo dq motion droid                # List animations for Droid
```

### Creature Types (ActorType Enum)

```bash
# Available creature types
cargo dq motion 0        # Human (ActorType::Human)
cargo dq motion 1        # PlayerLimb (ActorType::PlayerLimb)
cargo dq motion 2        # Droid (ActorType::Droid)
cargo dq motion 3        # Overlord (ActorType::Overlord)
cargo dq motion 4        # Arachnid (ActorType::Arachnid)
```

### Tag-Based Animation Queries

```bash
# Query with basic tags
cargo dq motion 0 +human +playspecmotion
cargo dq motion 0 +locomote

# Query with tag values (similar to spew files)
cargo dq motion 0 +cs:184           # Specific cutscene animation
cargo dq motion 0 +cs:116           # Another cutscene

# Multiple tags for specific animations
cargo dq motion 0 +playspecmotion +human --limit 10
```

### Motion Query Examples

```bash
# Find all human animations
cargo dq motion human +playspecmotion +human

# Find specific cutscene animations
cargo dq motion 0 +cs:184

# Explore droid animations
cargo dq motion droid +playspecmotion

# Limited results for quick exploration
cargo dq motion 0 --limit 5
```

### Motion Database Features

1. **Tag-Based Queries**: Uses the same hierarchical tag system as the original Dark Engine
2. **Creature Type Support**: Supports both numeric IDs (0, 1, 2...) and names (human, droid, etc.)
3. **Value Tags**: Supports tags with values like `+cs:184` for specific cutscenes
4. **Spew File Compatible**: Output format similar to original animation spew files
5. **Multiple Tags**: Combine multiple tags to find specific animation sets

### Understanding Motion Database Output

The motion database organizes animations hierarchically using tags:

- **`+playspecmotion`**: General animation category
- **`+human`**: Human-specific animations
- **`+cs:VALUE`**: Cutscene animations with specific IDs
- **`+locomote`**: Movement animations
- **Creature-specific tags**: `+midwife`, `+droid`, etc.

This system matches the tag database structure found in the original spew files and allows precise animation queries for debugging AI behavior and animation systems.

## Data Path Management

The project includes a centralized data path management system to handle platform-specific data locations:

### Using `shock2vr::paths::data_root()`

**ALWAYS use `shock2vr::paths::data_root()` instead of hardcoded "Data/" paths.**

```rust
use shock2vr::paths;

// ✅ Correct - uses data_root() helper
let motiondb_path = paths::data_root().join("motiondb.bin");
let error_msg = format!("File not found under {}/res/motions", paths::data_root().display());

// ❌ Incorrect - hardcoded paths
let motiondb_path = "Data/motiondb.bin";
let motiondb_path = "../../Data/motiondb.bin";
```

### How `data_root()` Works

- **Desktop**:
  1. First checks `DARK_ASSET_PATH` environment variable if set
  2. Then searches `["./Data", "../Data", "../../Data", "."]` for sentinel files (`shock2.gam`, `motiondb.bin`, etc.)
  3. Falls back to `"../../Data"` if no sentinel files found
- **Android**: Returns `/mnt/sdcard/shock2quest`

### Environment Variable

Set `DARK_ASSET_PATH` to point to your data directory for multi-repo development:

```bash
export DARK_ASSET_PATH=/path/to/your/shock2/data
cargo run -p dark_query -- entities
```

**Note**: The `engine` crate cannot depend on `shock2vr`, so `engine/src/gl_engine.rs` keeps its hardcoded path.

## Getting Help

- Check existing code for similar patterns
- Follow the incremental development process outlined above
- Focus on code analysis tasks:
  - Performance bottleneck identification
  - Code pattern consistency
  - Memory usage optimization
  - Rendering pipeline efficiency
- Use desktop runtime for rapid iteration and debugging
- **Use `dark_query` CLI tool** to understand entity relationships and debug complex interactions
- It may be necessary to create one-off CLI tools to exercise functionality - feel free to add these as part of the PR. This is especially useful when a change may require understanding the games metadata (ie, the .gam or .mis files) - using our existing parsing tools to query the data can help with understanding the format.

## Testing

- Make sure, when adding a test that exercises code in a PR, to do a _negative_ test first - it should fail without the necessary change. Then, validate the code change makes it green
