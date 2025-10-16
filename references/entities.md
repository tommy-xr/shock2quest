# System Shock 2 Entity System

## Overview

The System Shock 2 entity system is a sophisticated inheritance-based architecture that defines game objects through Templates, Properties, Links, and Scripts. This system enables complex object behaviors and relationships while maintaining data efficiency through hierarchical inheritance.

## Core Concepts

### Templates

Templates are the fundamental building blocks of entities. Each template has:

- **Template ID**: Unique integer identifier
- **Properties**: Data defining behavior and appearance
- **Links**: Relationships to other templates/entities
- **Inheritance**: Parent-child relationships via MetaProp links

Templates exist in two contexts:

1. **Gamesys Templates** (`shock2.gam`): Base definitions and common objects
2. **Mission Templates** (`.mis` files): Level-specific entities and overrides

### Properties (Props)

Properties define entity characteristics and behaviors. Each property has:

- **Chunk Name**: 8-character identifier (e.g., `P$Position`, `P$ModelName`)
- **Data**: Binary data specific to the property type
- **Inheritance**: Properties from parent templates are inherited and can be overridden

Common property categories:

- **Transform**: `PropPosition`, `PropScale`, `PropPhysState`
- **Rendering**: `PropModelName`, `PropRenderType`, `PropBitmapAnimation`
- **Physics**: `PropPhysType`, `PropPhysDimensions`, `PropPhysAttr`
- **Behavior**: `PropScripts`, `PropAI`, `PropFrobInfo`
- **Metadata**: `PropSymName`, `PropObjName`, `PropTemplateId`

### Links

Links define relationships between entities. Types include:

- **MetaProp Links** (`L$MetaProp`): Inheritance hierarchy (child → parent)
- **Behavioral Links**: `L$Contains`, `L$Flinderize`, `L$Corpse`
- **AI Links**: `L$AIWatchOb`, `L$AIProject`, `L$Projectil`
- **Utility Links**: `L$SwitchLin`, `L$TPath`, `L$GunFlash`

Links can have associated data stored in separate chunks (e.g., `LD$Contains`).

### Scripts

Scripts provide entity logic and are referenced by the `PropScripts` property. Scripts handle:

- Player interactions (frob, pickup, use)
- AI behaviors and responses
- Environmental triggers
- Object lifecycle management

## Data Flow Architecture

```
shock2.gam (Gamesys)
    ├── Base templates & properties
    ├── Common entity definitions
    └── Sound schemas & metadata
             │
             ▼
    Mission File (.mis)
    ├── Level-specific entities
    ├── Property overrides
    └── Spatial relationships
             │
             ▼
    merge_with_gamesys()
    ├── Combines gamesys + mission data
    ├── Resolves inheritance hierarchies
    └── Creates unified entity database
             │
             ▼
    Entity Instantiation
    ├── Creates shipyard entities
    ├── Applies inherited properties
    ├── Establishes entity links
    └── Initializes scripts & physics
```

## File Structure

### Gamesys File (`shock2.gam`)

```
GAMESYS CHUNKS:
├── Property Chunks (P$*)
│   ├── P$Position    - Entity positions
│   ├── P$ModelName   - 3D model references
│   ├── P$Scripts     - Script assignments
│   └── ...
├── Link Chunks (L$*)
│   ├── L$MetaProp    - Inheritance links
│   ├── L$Contains    - Container relationships
│   └── ...
├── Link Data (LD$*)
│   ├── LD$Contains   - Container link data
│   ├── LD$AIProjec   - AI projectile data
│   └── ...
└── Metadata
    ├── Sound schemas
    ├── Environmental audio
    └── Speech databases
```

### Mission File (`.mis`)

```
MISSION CHUNKS:
├── World Geometry (WREXT/WRRGB)
├── Entity Data
│   ├── Property chunks (P$*)
│   ├── Link chunks (L$*)
│   └── Link data (LD$*)
├── Level Metadata
│   ├── OBJ_MAP - Template name mappings
│   ├── Room database
│   └── Song parameters
└── Rendering Data
    ├── Texture lists
    ├── Lightmaps
    └── BSP tree
```

## Inheritance Mechanism

### MetaProp Links

The inheritance system uses `L$MetaProp` links to establish parent-child relationships:

```rust
Link {
    src: child_template_id,    // Child template
    dest: parent_template_id,  // Parent template
    flavor: 0,                 // Link type indicator
}
```

### Property Resolution

When instantiating an entity:

1. **Collect Ancestors**: Traverse MetaProp links to build inheritance chain
2. **Apply Properties**: Process properties from root → leaf order
3. **Handle Overrides**: Child properties override parent properties
4. **Special Cases**: Some properties (like Scripts) use merge logic

```rust
fn get_ancestors(hierarchy: &HashMap<i32, Vec<i32>>, id: &i32) -> Vec<i32> {
    // Recursively traverse parents
    // Returns ordered list: [root_ancestor, ..., direct_parent]
}
```

### Example: Entity Creation

```rust
// Template 1001: "Base Object" (root)
//   ├── P$Position: (0,0,0)
//   └── P$Scripts: ["BaseScript"]

// Template 1002: "Weapon" → inherits from 1001
//   ├── P$ModelName: "sword.bin"
//   └── P$Scripts: ["WeaponScript"] (inherits: true)

// Template 1003: "Magic Sword" → inherits from 1002
//   ├── P$Position: (10,5,3)  // Overrides base position
//   └── P$Scripts: ["MagicScript"] (inherits: true)

// Final resolved properties for Magic Sword:
//   ├── P$Position: (10,5,3)      // From 1003 (override)
//   ├── P$ModelName: "sword.bin"  // From 1002 (inherited)
//   └── P$Scripts: ["BaseScript", "WeaponScript", "MagicScript"] // Merged
```

## Code Architecture

### Key Files

#### Entity Parsing

- `dark/src/ss2_entity_info.rs`: Core entity data structures and parsing
- `dark/src/gamesys/gamesys.rs`: Gamesys file reading
- `dark/src/properties/mod.rs`: Property definitions and parsing

#### Entity Instantiation

- `shock2vr/src/mission/mod.rs`: Mission loading and entity merging
- `shock2vr/src/mission/entity_creator.rs`: Entity instantiation logic
- `shock2vr/src/mission/entity_populator/`: Entity population strategies

### Core Data Structures

```rust
pub struct SystemShock2EntityInfo {
    pub entity_to_properties: HashMap<i32, Vec<Rc<Box<dyn Property>>>>,
    pub template_to_links: HashMap<i32, TemplateLinks>,
    pub link_metaprops: Vec<Link>,
    hierarchy: HashMap<i32, Vec<i32>>,  // Template inheritance tree
}

pub struct TemplateLinks {
    pub to_links: Vec<ToTemplateLink>,
}

pub struct ToTemplateLink {
    pub to_template_id: i32,
    pub link: Link,  // Specific link type with data
}
```

### Entity Creation Pipeline

1. **Parse Gamesys**: Load `shock2.gam` → `SystemShock2EntityInfo`
2. **Parse Mission**: Load `.mis` file → Mission-specific `SystemShock2EntityInfo`
3. **Merge**: `merge_with_gamesys()` → Combined entity database
4. **Instantiate**: Create shipyard entities with resolved properties
5. **Materialize**: Create models, physics bodies, and scripts

## Common Patterns

### Entity Queries

```rust
// Find all entities with a specific property
world.run(|v_model: View<PropModelName>| {
    for (entity_id, model) in v_model.iter().with_id() {
        println!("Entity {} has model: {}", entity_id, model.0);
    }
});

// Get entity's template hierarchy
let ancestors = get_ancestors(hierarchy, &template_id);
```

### Property Access

```rust
// Read property with fallback
let position = v_position.get(entity_id)
    .unwrap_or(PropPosition {
        position: Vector3::zero(),
        rotation: Quaternion::identity(),
        cell: 0
    });
```

### Link Traversal

```rust
// Find all contained objects
if let Ok(links) = v_links.get(entity_id) {
    for link in &links.to_links {
        if let Link::Contains(_) = link.link {
            if let Some(contained_entity) = link.to_entity_id {
                // Process contained entity
            }
        }
    }
}
```

## Debugging Tips

### Common Issues

1. **Missing Properties**: Check inheritance chain and gamesys merging
2. **Broken Links**: Verify template IDs exist in target context
3. **Script Errors**: Ensure script files exist and are properly registered
4. **Physics Problems**: Check `PropPhysType` and `PropPhysDimensions`

### Useful Debugging Commands

```rust
// Print entity's complete property set
for (template_id, props) in &entity_info.entity_to_properties {
    println!("Template {}: {} properties", template_id, props.len());
}

// Trace inheritance hierarchy
let ancestors = get_ancestors(hierarchy, &template_id);
println!("Inheritance: {:?}", ancestors);

// Find entities by name
world.run(|v_name: View<PropSymName>| {
    for (id, name) in v_name.iter().with_id() {
        if name.0.contains("search_term") {
            println!("Found: {} ({})", name.0, id);
        }
    }
});
```

## Performance Considerations

- **Property Inheritance**: Resolved at instantiation time, not runtime
- **Link Resolution**: Template links converted to entity links during creation
- **Memory Usage**: Properties stored as `Rc<Box<dyn Property>>` for sharing
- **Entity Limits**: Typical missions have 1000-5000 entities
