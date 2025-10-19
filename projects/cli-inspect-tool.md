# CLI Inspector Tool Project

## Overview

A low-level CLI inspector tool for raw System Shock 2 asset file analysis. This tool focuses on file format parsing, data extraction, and validation - providing detailed information about the binary structure and content of asset files. It's designed to make it easy for LLMs and developers to test and exercise the asset parsing functionality across multiple file formats.

## Scope & Distinction

This tool is specifically focused on **raw file format inspection** and complements (but does not overlap with) the separate Entity Query Tool project:

### CLI Inspector Tool (This Project)
- **Focus**: Raw file format parsing and structure analysis
- **Level**: Low-level, binary format focused
- **Purpose**: Understanding file layouts, debugging parsers, asset validation
- **Use Cases**:
  - "What chunks are in this .mis file?"
  - "How many vertices does this .bin model have?"
  - "What's the header structure of this .gam file?"
  - "Are there any parsing errors in this asset?"
- **Output**: File structure, raw data counts, format validation, parsing diagnostics

### Entity Query Tool (Separate Project)
- **Focus**: Semantic entity relationships and gameplay data
- **Level**: High-level, game logic focused
- **Purpose**: Entity inheritance, property analysis, gameplay debugging
- **Use Cases**:
  - "Which entities inherit from BaseWeapon?"
  - "What properties does this entity template have?"
  - "Show me all Contains relationships in this mission"
  - "Validate entity inheritance chains"
- **Output**: Entity hierarchies, property inheritance, semantic relationships

The CLI inspector provides the foundation that enables higher-level tools like the entity query system by ensuring the underlying file parsing is robust and well-understood.

## Current State Analysis

### Existing Tools
- **`bin_obj_inspector`**: Already exists and inspects .bin files specifically
- **Main tool (`main.rs`)**: Visual renderer/viewer for models and animations
- **Tool crate**: `ss2tool` in `runtimes/tool/`

### Supported File Formats
The codebase already has robust parsing capabilities for:

| File Type | Parser Module | Content Type | Status |
|-----------|---------------|--------------|---------|
| `.bin` | `ss2_bin_obj_loader`, `ss2_bin_ai_loader` | Static objects (LGMD), Animated meshes (LGMM) | ✅ Supported |
| `.obj` | Via `.bin` format | 3D Objects | ✅ Supported |
| `.mis` | `mission::read()` | Mission/Level files | ✅ Supported |
| `.cal` | `ss2_cal_loader` | Skeleton/Animation data | ✅ Supported |
| `.gam` | `gamesys::read()` | Game system definitions | ✅ Supported |
| `.pcx` | `texture_importer` | Textures | ✅ Supported |
| `.wav` | `audio_importer` | Audio files | ✅ Supported |
| `.fon` | `font_importer` | Fonts | ✅ Supported |

### Asset Pipeline Architecture
- **AssetImporter System**: Standardized loading interface
- **AssetCache**: Caching and dependency resolution
- **Importers**: Specialized processors for each file type
  - `MODELS_IMPORTER` - Handles .bin files
  - `TEXTURE_IMPORTER` - Handles .pcx files
  - `SKELETON_IMPORTER` - Handles .cal files
  - And more...

## Proposed Design

### Architecture Goals
1. **Unified Interface**: Single `inspect` command for all file types
2. **Extensible**: Easy to add new file format support
3. **LLM-Friendly**: Structured, parseable output formats
4. **Asset Pipeline Integration**: Leverage existing importers and parsers
5. **Raw Data Focus**: File structure and binary content analysis (not semantic relationships)
6. **Parser Validation**: Comprehensive format verification and error detection

### Command Structure

```bash
# Basic usage
cargo run --bin inspect <file_path>

# With output format control
cargo run --bin inspect <file_path> --format json
cargo run --bin inspect <file_path> --format human
cargo run --bin inspect <file_path> --format summary

# With specific analysis depth
cargo run --bin inspect <file_path> --depth basic|detailed|full

# With filtering
cargo run --bin inspect <file_path> --show-only entities
cargo run --bin inspect <file_path> --show-only geometry
cargo run --bin inspect <file_path> --show-only textures
```

### Output Formats

#### 1. Human-Readable (Default)
```
=== System Shock 2 Asset Inspector ===
File: medsci1.mis
Type: Mission File
Size: 2.4 MB

--- HEADER INFORMATION ---
Version: 2.6
Chunk Count: 47

--- WORLD GEOMETRY ---
Cells: 234
Polygons: 15,420
Vertices: 8,901
Textures: 127

--- ENTITIES (RAW COUNTS) ---
Template definitions: 89
Instance definitions: 445
Property chunks: 1,234

--- LINKS (RAW COUNTS) ---
MetaProp links: 67
Contains links: 234
Flinderize links: 12
...

--- LIGHTMAPS ---
Atlas Size: 4096x4096
Lightmaps: 892
```

#### 2. JSON Format
```json
{
  "file_info": {
    "path": "medsci1.mis",
    "type": "mission",
    "size_bytes": 2454016
  },
  "format_info": {
    "version": "2.6",
    "chunk_count": 47,
    "chunks": [
      {"name": "WREXT", "offset": 1024, "length": 12340},
      ...
    ]
  },
  "geometry": {
    "cell_count": 234,
    "polygon_count": 15420,
    "vertex_count": 8901,
    "texture_count": 127
  },
  "entities": {
    "template_count": 89,
    "instance_count": 445,
    "property_chunk_count": 1234,
    "raw_template_data": [
      {"id": 1, "chunk_size": 256, "property_chunks": 5},
      ...
    ]
  }
}
```

#### 3. Summary Format
```
medsci1.mis: Mission file, 234 cells, 445 entities, 127 textures
```

### Implementation Plan

#### Phase 1: Core Infrastructure
1. **Create new binary**: `src/bin/inspect.rs`
2. **File type detection**: Automatic format recognition
3. **Basic output structure**: Human-readable format
4. **Error handling**: Graceful failure modes

#### Phase 2: File Format Support
1. **Mission files (.mis)**: Full level analysis
2. **Binary files (.bin)**: Model and object inspection
3. **Gamesys files (.gam)**: Entity template analysis
4. **Skeleton files (.cal)**: Animation structure

#### Phase 3: Enhanced Features
1. **JSON output**: Machine-readable format
2. **Filtering options**: Specific data extraction
3. **Validation**: File integrity checking
4. **Dependencies**: Asset reference tracking

#### Phase 4: Advanced Analysis
1. **Cross-references**: Entity relationships
2. **Statistics**: Performance metrics
3. **Comparison**: File diff capabilities
4. **Export**: Data extraction utilities

### Code Architecture

```rust
// Main inspector structure
pub struct AssetInspector {
    asset_cache: AssetCache,
    output_format: OutputFormat,
    analysis_depth: AnalysisDepth,
}

// File type detection
pub enum AssetType {
    Mission,     // .mis
    BinaryModel, // .bin
    Gamesys,     // .gam
    Skeleton,    // .cal
    Texture,     // .pcx
    Audio,       // .wav
    Font,        // .fon
    Unknown,
}

// Analysis results
pub struct InspectionResult {
    file_info: FileInfo,
    format_info: FormatInfo,
    content_analysis: ContentAnalysis,
    validation_results: Vec<ValidationIssue>,
}

// Specialized inspectors for each format
trait FormatInspector {
    fn inspect(&self, reader: &mut dyn ReadableAndSeekable) -> Result<ContentAnalysis>;
    fn validate(&self, content: &ContentAnalysis) -> Vec<ValidationIssue>;
}
```

### File-Specific Inspection Details

#### Mission Files (.mis)
- **Chunk structure**: Complete TOC analysis
- **World geometry**: Cell/polygon/vertex counts
- **Entity data**: Template hierarchy, instances, properties
- **Lightmaps**: Atlas information and coverage
- **BSP tree**: Spatial subdivision structure
- **Texture references**: Asset dependencies

#### Binary Models (.bin)
- **Header information**: Type (LGMD/LGMM), version
- **Geometry data**: Vertex counts, normal vectors
- **Animation support**: Joint structure (for LGMM)
- **Collision data**: Physics boundaries
- **Texture mapping**: UV coordinates

#### Gamesys Files (.gam)
- **Entity templates**: Complete hierarchy
- **Property definitions**: All P$ chunks
- **Link types**: Relationship definitions
- **Script assignments**: Behavior mappings

#### Skeleton Files (.cal)
- **Joint hierarchy**: Bone structure
- **Animation compatibility**: Supported motions
- **Binding information**: Mesh attachment points

### Integration Points

#### Existing Asset Pipeline Usage
```rust
// Leverage existing importers
let model = asset_cache.get(&MODELS_IMPORTER, file_path);
let texture_info = asset_cache.get(&TEXTURE_IMPORTER, texture_path);
let skeleton = asset_cache.get(&SKELETON_IMPORTER, skeleton_path);

// Use existing parsers directly
let mission_data = mission::read(&mut reader, &gamesys, &links, ...);
let bin_header = ss2_bin_header::read(&mut reader);
```

#### Error Handling Strategy
- **Graceful degradation**: Partial parsing on errors
- **Detailed diagnostics**: File corruption detection
- **Recovery suggestions**: Common fix recommendations

### Testing Strategy

#### Unit Tests
- **File format parsing**: Each importer validation
- **Output generation**: Format consistency
- **Error conditions**: Malformed file handling

#### Integration Tests
- **Real asset files**: Known good samples
- **Performance testing**: Large file handling
- **Cross-platform**: Consistent behavior

#### Test Data
- **Minimal examples**: Simple valid files
- **Edge cases**: Complex scenarios
- **Corrupted files**: Error condition testing

### Documentation

#### Usage Examples
```bash
# Inspect a mission file structure
cargo run --bin inspect Data/medsci1.mis

# Get JSON output for scripting
cargo run --bin inspect Data/models/pistol.bin --format json

# Quick summary of multiple files
for file in Data/models/*.bin; do
    cargo run --bin inspect "$file" --format summary
done

# Detailed binary structure analysis
cargo run --bin inspect Data/shock2.gam --show-only chunks --depth full

# Validate file parsing
cargo run --bin inspect Data/models/damaged.bin --validate
```

#### LLM Integration Patterns
```bash
# Discover file structure and chunks
cargo run --bin inspect unknown_file.bin --format json | jq '.format_info'

# Extract raw binary structure information
cargo run --bin inspect level.mis --show-only chunks --format json

# Validate parsing and detect corruption
cargo run --bin inspect asset.bin --validate --depth full | grep -E "(ERROR|WARNING)"

# Compare file format variations
cargo run --bin inspect file1.gam --format json > format1.json
cargo run --bin inspect file2.gam --format json > format2.json
```

### Implementation Dependencies

#### Required Crates
- All existing `dark` module functionality
- `engine::assets` for asset pipeline integration
- `serde_json` for JSON output
- `clap` for command-line parsing
- `tracing` for diagnostic logging

#### New Dependencies
- `serde` - JSON serialization
- `clap` - CLI argument parsing
- `indicatif` - Progress bars for large files
- `colored` - Terminal color output

### Future Enhancements

#### Advanced Features
1. **Asset validation**: Comprehensive integrity checking
2. **Performance profiling**: Loading time analysis
3. **Dependency mapping**: Asset reference graphs
4. **Batch processing**: Multiple file analysis
5. **Export utilities**: Data extraction tools

#### Potential Extensions
1. **Web interface**: Browser-based inspector
2. **Plugin system**: Custom analysis modules
3. **Comparison tools**: File difference analysis
4. **Asset optimization**: Efficiency recommendations

## Implementation Timeline

| Phase | Duration | Deliverables |
|-------|----------|--------------|
| 1 | 1-2 days | Basic CLI tool with file type detection |
| 2 | 2-3 days | Support for .mis, .bin, .gam files |
| 3 | 2-3 days | JSON output, filtering, validation |
| 4 | 3-4 days | Advanced analysis features |

## Success Metrics

1. **Functionality**: Can inspect all major SS2 file types
2. **Reliability**: Handles corrupted/malformed files gracefully
3. **Performance**: Processes large mission files in <5 seconds
4. **Usability**: LLMs can easily extract needed information
5. **Maintainability**: Easy to add new file format support

## Risk Assessment

### Technical Risks
- **Parser complexity**: Mission file parsing is intricate
- **Memory usage**: Large files may cause issues
- **Compatibility**: Cross-platform file handling

### Mitigation Strategies
- **Incremental development**: Start with simple formats
- **Streaming parsing**: Avoid loading entire files
- **Comprehensive testing**: Multiple platforms and file types

## Conclusion

This CLI inspector tool will provide a unified, low-level interface to the robust asset parsing capabilities already present in the Shock2Quest codebase. By focusing on raw file format analysis and binary structure inspection, it complements the higher-level Entity Query Tool while serving a distinct purpose.

The tool fills a critical gap for:
- **Format-level debugging**: Understanding binary file structures and parsing correctness
- **LLM-assisted development**: Providing structured data about file contents for AI analysis
- **Parser validation**: Ensuring asset parsing robustness across different file variations
- **Asset integrity checking**: Detecting corruption or format inconsistencies

Together with the Entity Query Tool, this creates a complete toolkit for System Shock 2 asset analysis - from low-level binary inspection to high-level semantic relationships.