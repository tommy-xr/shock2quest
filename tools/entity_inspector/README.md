# Entity Inspector

A command-line tool for inspecting and analyzing System Shock 2 entity data from gamesys (.gam) and mission (.mis) files.

## Features

- **File Format Support**: Parse both gamesys and mission files
- **Entity Inspection**: Examine individual entities by template ID or name
- **Data Export**: Export entity data in JSON, CSV, or debug formats
- **Query System**: Filter entities by template ID ranges
- **Validation**: Basic file validation and integrity checking
- **Auto-Discovery**: Automatically locate gamesys files for mission analysis

## Installation

Build from the workspace root:

```bash
cargo build --bin entity_inspector
```

## Usage

### Basic Entity Inspection

```bash
# Inspect a specific entity by template ID
entity_inspector inspect --file Data/shock2.gam --template 1234 --show-properties

# Load and display database overview
entity_inspector inspect --file Data/medsci1.mis
```

### Export Data

```bash
# Export all entities to JSON
entity_inspector export --file Data/shock2.gam --format json --output entities.json

# Export entity list to CSV with property counts
entity_inspector export --file Data/shock2.gam --format csv --properties --output entities.csv
```

### Query Entities

```bash
# Find entities in a template ID range
entity_inspector query --file Data/shock2.gam --template-range "1000..2000"
```

### Validate Files

```bash
# Basic file validation
entity_inspector validate --file Data/shock2.gam

# Check inheritance and links (planned feature)
entity_inspector validate --file Data/shock2.gam --check-inheritance --check-links
```

## File Format Support

- **Gamesys Files (.gam)**: Core game entity definitions
- **Mission Files (.mis)**: Level-specific entity overrides and additions

The tool automatically searches for the required gamesys file when loading mission files, checking the same directory and parent directories for common names like `shock2.gam`.

## Development Status

This is Phase 1 implementation with basic functionality. Future enhancements planned:

- Full property inspection and display
- Complete inheritance hierarchy visualization
- Name-based entity search
- Advanced property-based queries
- Link relationship analysis
- Enhanced validation features

## Architecture

The tool is built on the existing `dark` crate parsing infrastructure:

- `database.rs` - Entity data loading and management
- `commands.rs` - CLI command implementations
- `query.rs` - Entity filtering and search
- `formatters.rs` - Output formatting
- `error.rs` - Comprehensive error handling

## Contributing

This tool is part of the larger Shock2Quest project. See the main project documentation for contribution guidelines.