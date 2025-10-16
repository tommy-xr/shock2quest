# Entity System Improvements & Query Tool

## Project Overview

This project aims to improve the entity system implementation and create powerful debugging/analysis tools for System Shock 2's complex entity relationships. The work is divided into three main areas: critical fixes, tooling improvements, and enhanced debugging capabilities.

## Phase 1: Critical Entity System Fixes

### 1.1 MetaProp Link Implementation Review

**Issue**: Current inheritance system may have incorrect traversal order and multiple inheritance handling.

**Tasks**:
- [ ] Audit current `calculate_hierarchy()` and `get_ancestors()` functions
- [ ] Verify inheritance direction (child→parent vs parent→child)
- [ ] Fix traversal order to ensure proper property resolution
- [ ] Add comprehensive tests for inheritance chains
- [ ] Document expected vs actual behavior with real game entities

**Implementation**:
```rust
// Proposed fix for get_ancestors()
pub fn get_ancestors_corrected(hierarchy: &HashMap<i32, Vec<i32>>, id: &i32) -> Vec<i32> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = vec![*id];

    while let Some(current) = stack.pop() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current);

        if let Some(parents) = hierarchy.get(&current) {
            // Add parents in reverse order for correct inheritance
            for &parent in parents.iter().rev() {
                if !visited.contains(&parent) {
                    result.push(parent);
                    stack.push(parent);
                }
            }
        }
    }

    result.reverse(); // Root ancestors first
    result
}
```

**Testing Strategy**:
- Create test cases with known inheritance chains
- Verify property resolution order matches Dark Engine behavior
- Test with actual game entities (weapons, creatures, items)

### 1.2 Property Inheritance Validation

**Issue**: Property override and merge logic may not match Dark Engine behavior.

**Tasks**:
- [ ] Audit `WrappedProperty::initialize()` accumulator logic
- [ ] Fix script inheritance (should merge, not override)
- [ ] Ensure proper property override behavior
- [ ] Add validation for property conflicts

**Implementation**:
```rust
// Enhanced property resolution with logging
impl<C> Property for WrappedProperty<C> {
    fn initialize(&self, world: &mut World, entity: EntityId) {
        let view: ViewMut<C> = world.borrow().unwrap();
        let maybe_previous = view.get(entity);

        let final_value = match maybe_previous {
            Ok(previous) => {
                trace!("Property {}: merging with existing value", type_name::<C>());
                (self.accumulator)(previous.clone(), self.inner_property.clone())
            }
            Err(_) => {
                trace!("Property {}: setting initial value", type_name::<C>());
                self.inner_property.clone()
            }
        };

        drop(view);
        world.add_component(entity, final_value);
    }
}
```

### 1.3 Error Handling Improvements

**Issue**: Property parsing errors are not well-handled, leading to silent failures.

**Tasks**:
- [ ] Add comprehensive error types for entity system
- [ ] Improve error reporting in property parsing
- [ ] Add validation for template ID references
- [ ] Handle missing templates gracefully

**Implementation**:
```rust
#[derive(Debug, thiserror::Error)]
pub enum EntitySystemError {
    #[error("Template {0} not found")]
    TemplateNotFound(i32),

    #[error("Property parsing failed for {property}: {source}")]
    PropertyParsingError { property: String, source: Box<dyn std::error::Error> },

    #[error("Circular inheritance detected: {0:?}")]
    CircularInheritance(Vec<i32>),

    #[error("Invalid MetaProp link: {src} -> {dest}")]
    InvalidMetaPropLink { src: i32, dest: i32 },
}
```

## Phase 2: Entity Parsing & Query Tool

### 2.1 Core CLI Tool Architecture

**Goal**: Create a standalone CLI tool to inspect and query System Shock 2 entity data.

**Features**:
- Parse gamesys (shock2.gam) and mission files (.mis)
- Query entities by name, template ID, or properties
- Trace inheritance hierarchies
- Analyze entity relationships
- Export entity data in various formats

**Implementation Structure**:
```
src/bin/entity_inspector.rs
├── commands/
│   ├── inspect.rs      - Inspect specific entities
│   ├── query.rs        - Search/filter entities
│   ├── hierarchy.rs    - Show inheritance trees
│   ├── properties.rs   - List/analyze properties
│   ├── links.rs        - Analyze entity relationships
│   └── export.rs       - Export data (JSON, CSV, etc.)
├── parsers/
│   ├── gamesys.rs      - Gamesys file parsing
│   ├── mission.rs      - Mission file parsing
│   └── common.rs       - Shared parsing utilities
├── query/
│   ├── engine.rs       - Query execution engine
│   ├── filters.rs      - Property/link filters
│   └── formatters.rs   - Output formatting
└── main.rs            - CLI interface
```

### 2.2 Command Line Interface

```bash
# Basic inspection
entity_inspector inspect --file shock2.gam --template 1234
entity_inspector inspect --file medsci1.mis --name "Security Camera"

# Hierarchy analysis
entity_inspector hierarchy --file shock2.gam --template 1234 --show-properties
entity_inspector hierarchy --file shock2.gam --roots  # Show all root templates

# Property queries
entity_inspector query --file shock2.gam --property "P$ModelName=laserpis.bin"
entity_inspector query --file medsci1.mis --has-property "P$Scripts"

# Link analysis
entity_inspector links --file shock2.gam --link-type MetaProp
entity_inspector links --file medsci1.mis --from-template 1234

# Data export
entity_inspector export --file shock2.gam --format json --output entities.json
entity_inspector export --file medsci1.mis --format csv --properties --output mission_props.csv

# Validation
entity_inspector validate --file shock2.gam --check-inheritance
entity_inspector validate --file medsci1.mis --check-links
```

### 2.3 Query Engine Implementation

```rust
pub struct EntityQuery {
    pub template_filter: Option<TemplateFilter>,
    pub property_filters: Vec<PropertyFilter>,
    pub link_filters: Vec<LinkFilter>,
    pub output_format: OutputFormat,
}

pub enum TemplateFilter {
    ById(i32),
    ByName(String),
    ByRange(Range<i32>),
    HasAncestor(i32),
}

pub enum PropertyFilter {
    HasProperty(String),
    PropertyEquals(String, String),
    PropertyMatches(String, Regex),
}

pub enum LinkFilter {
    HasLinkType(String),
    LinksTo(i32),
    LinksFrom(i32),
}

pub enum OutputFormat {
    Table,
    Json,
    Csv,
    Tree,
    Debug,
}
```

### 2.4 Integration with Existing Codebase

**Approach**: Leverage existing parsers while adding query capabilities.

```rust
// Reuse existing parsing infrastructure
use dark::gamesys;
use dark::ss2_entity_info;
use dark::properties;

pub struct EntityDatabase {
    pub gamesys: Option<dark::gamesys::Gamesys>,
    pub mission_info: Option<dark::ss2_entity_info::SystemShock2EntityInfo>,
    pub merged_info: dark::ss2_entity_info::SystemShock2EntityInfo,
}

impl EntityDatabase {
    pub fn from_gamesys(path: &Path) -> Result<Self, EntitySystemError> {
        // Use existing gamesys parsing
    }

    pub fn from_mission(path: &Path, gamesys: &Gamesys) -> Result<Self, EntitySystemError> {
        // Use existing mission parsing + merging
    }

    pub fn query(&self, query: &EntityQuery) -> QueryResult {
        // Execute query against parsed data
    }
}
```

## Phase 3: Enhanced Debugging & Tooling

### 3.1 Runtime Entity Inspector

**Goal**: Add runtime debugging capabilities to the game itself.

**Features**:
- Entity browser UI (debug mode)
- Real-time property inspection
- Link visualization
- Inheritance tree display

**Implementation**:
```rust
// Add to debug GUI system
pub struct EntityInspectorGui {
    selected_entity: Option<EntityId>,
    show_hierarchy: bool,
    property_filter: String,
}

impl EntityInspectorGui {
    pub fn render(&mut self, world: &World, entity_info: &SystemShock2EntityInfo) {
        // ImGui-based entity browser
        if imgui::CollapsingHeader::new("Entity Inspector").build() {
            self.render_entity_list(world);
            self.render_entity_details(world, entity_info);
            self.render_hierarchy_view(entity_info);
        }
    }
}
```

### 3.2 Performance Optimization

**Issues**: Entity system performance bottlenecks.

**Tasks**:
- [ ] Profile entity creation performance
- [ ] Optimize inheritance resolution
- [ ] Cache frequently accessed hierarchies
- [ ] Reduce memory allocations in property systems

**Implementation**:
```rust
// Cached hierarchy for performance
pub struct CachedHierarchy {
    cache: HashMap<i32, Vec<i32>>,
    raw_hierarchy: HashMap<i32, Vec<i32>>,
}

impl CachedHierarchy {
    pub fn get_ancestors(&mut self, template_id: i32) -> &Vec<i32> {
        self.cache.entry(template_id).or_insert_with(|| {
            calculate_ancestors(&self.raw_hierarchy, template_id)
        })
    }
}
```

### 3.3 Enhanced Validation

**Goal**: Detect and report entity system inconsistencies.

**Features**:
- Circular inheritance detection
- Missing template reference validation
- Property consistency checks
- Link integrity verification

**Implementation**:
```rust
pub struct EntityValidator {
    errors: Vec<ValidationError>,
    warnings: Vec<ValidationWarning>,
}

impl EntityValidator {
    pub fn validate_inheritance(&mut self, entity_info: &SystemShock2EntityInfo) {
        // Check for cycles, missing templates, etc.
    }

    pub fn validate_links(&mut self, entity_info: &SystemShock2EntityInfo) {
        // Verify link targets exist
    }

    pub fn validate_properties(&mut self, entity_info: &SystemShock2EntityInfo) {
        // Check property consistency
    }
}
```

## Implementation Timeline

### Phase 1: Critical Fixes (2-3 weeks)
- **Week 1**: MetaProp link audit and fixes
- **Week 2**: Property inheritance validation
- **Week 3**: Error handling improvements and testing

### Phase 2: Query Tool (4-5 weeks)
- **Week 1**: CLI framework and basic parsing
- **Week 2**: Query engine implementation
- **Week 3**: Output formatting and export features
- **Week 4**: Integration testing with real game files
- **Week 5**: Documentation and polish

### Phase 3: Enhanced Tooling (3-4 weeks)
- **Week 1**: Runtime inspector GUI
- **Week 2**: Performance optimizations
- **Week 3**: Validation system
- **Week 4**: Integration and testing

## Dependencies & Requirements

### Technical Dependencies
- Existing dark engine parsing infrastructure
- clap for CLI argument parsing
- serde for data serialization
- regex for pattern matching
- console/indicatif for CLI UX

### Testing Requirements
- Unit tests for all new functionality
- Integration tests with real game files
- Performance benchmarks
- Validation against known good data

### Documentation Requirements
- CLI tool usage documentation
- API documentation for new modules
- Examples and tutorials
- Migration guide for any breaking changes

## Success Criteria

### Phase 1 Success Metrics
- [ ] All existing entity tests pass
- [ ] Inheritance resolution matches expected behavior
- [ ] No more silent property parsing failures
- [ ] Performance maintains or improves

### Phase 2 Success Metrics
- [ ] CLI tool can parse all game files without errors
- [ ] Query performance is acceptable (< 1s for typical queries)
- [ ] Tool provides actionable debugging information
- [ ] Export formats are complete and accurate

### Phase 3 Success Metrics
- [ ] Runtime debugging tools are usable and helpful
- [ ] Entity system performance is optimized
- [ ] Validation catches real issues in game data
- [ ] Developer productivity is measurably improved

## Risk Mitigation

### Technical Risks
- **File format changes**: Extensive testing with multiple game versions
- **Performance regression**: Continuous benchmarking during development
- **Compatibility issues**: Maintain backward compatibility with existing APIs

### Project Risks
- **Scope creep**: Clearly defined phases with specific deliverables
- **Timeline slippage**: Buffer time built into estimates
- **Quality issues**: Comprehensive testing strategy at each phase

## Future Enhancements

### Post-Launch Improvements
- Visual entity relationship graphs
- Entity diff tools for comparing game versions
- Integration with level editors
- Automated entity validation in CI/CD
- Entity scripting language/DSL
- Performance profiling tools

### Community Features
- Export formats for modding tools
- Documentation generation
- Entity database web interface
- Community entity repositories