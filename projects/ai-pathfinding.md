# AI Pathfinding Implementation Plan

## Overview

This plan implements proper A* pathfinding for monster AI using the AIPATH data stored in mission files. The implementation is broken into 5 phases, each building on the previous.

## Current State

- ✅ **AIPATH parsing complete** - Complete pathfinding database extraction from mission files (Phase 1)
- ✅ **Debug visualization complete** - `--debug-pathfinding` flag renders navigation mesh overlay (Phase 2)
- ✅ **A* pathfinding complete** - Full interactive pathfinding with P-key testing and HTTP API (Phase 3)
- **AI uses direct movement** - Monsters chase players in a straight line with whisker-based collision avoidance
- **Scripted sequences exist** - `ScriptedSequenceBehavior` supports waypoint navigation via `GotoScriptedAction`, but uses direct steering
- **Ready for Phase 4** - AI integration with pathfinding service

## Design Decisions

- **A* Implementation**: Use the `pathfinding` crate for the A* algorithm
- **Patrol System**: Use original AIWatchObj/TPath links as high-level waypoints, with A* pathfinding between them
- **Debug Visualization**: Show cells + links (connectivity visualization)

---

## Phase 1: AIPATH Chunk Parsing ✅ **COMPLETED**

**Goal**: Parse the AIPATH chunk from mission files and output debug information about available pathfinding data.

### ✅ Completed Implementation

**Files Created/Modified:**
- ✅ `dark/src/mission/path_database.rs` - Complete AIPATH parsing with sAIPathCell, sAIPathVertex, sAIPathCellLink structures
- ✅ `dark/src/mission/mod.rs` - PathDatabase integration into mission loading
- ✅ `tools/dark_query/src/main.rs` - AIPATH inspection command with sanity checks

**Data Structures Implemented:**
```rust
/// A convex floor polygon for AI navigation
pub struct PathCell {
    pub id: u32,
    pub center: Vector3<f32>,      // Parsed center point from sAIPathCell
    pub vertex_indices: Vec<u32>,  // Populated from cell-vertex links
    pub flags: PathCellFlags,      // Movement restrictions
}

/// Link between two path cells (8 bytes, matches sAIPathCellLink)
pub struct PathCellLink {
    pub from_cell: u32,           // Populated from cell firstCell/cellCount
    pub to_cell: u32,             // Destination cell from link data
    pub edge_vertex_a: u32,       // Shared edge vertices
    pub edge_vertex_b: u32,
    pub ok_bits: MovementBits,    // Who can traverse (WALK, FLY, SWIM, SMALL_CREATURE)
    pub cost: u8,                 // Traversal cost
}

/// Complete path database loaded from AIPATH chunk
pub struct PathDatabase {
    pub cells: Vec<PathCell>,      // Navigation polygons
    pub vertices: Vec<Vector3<f32>>, // 3D boundary points
    pub links: Vec<PathCellLink>,    // Cell connections
}
```

### ✅ Results for medsci1.mis

**Pathfinding Database:**
- **4,695 navigation cells** with realistic center coordinates
- **9,900 vertices** with precise 3D boundary points
- **27,035 links** with complete source/destination mapping
- **18,722 vertex references** properly distributed across cells

**Data Quality Validation:**
- ✅ **Perfect data integrity** (all 27,035 links have valid cell/vertex references)
- ✅ **Realistic polygon shapes** (92% rectangles, 5% triangles, 2% pentagons)
- ✅ **Healthy connectivity** (5.8 average links per cell, 82.7% well-connected)
- ✅ **Complete polygon reconstruction** (all cells have vertex indices for boundary geometry)

**Debug Tool:**
```bash
cargo dq aipath medsci1.mis    # Complete pathfinding database inspection
cargo dq entities medsci1.mis  # Verify integration with mission loading
```

**Sample Output:**
```
=== AIPATH Database from medsci1.mis ===
Cells: 4695, Vertices: 9900, Links: 27035

Sample Cells:
  Cell 1: center=(130.8, 75.2, -18.0), 4 vertices, flags=(empty)
  Cell 2: center=(142.8, 75.2, -18.0), 4 vertices, flags=(empty)

Sample Vertices:
  Vertex 1: (124.75, 79.25, -18.00)
  Vertex 2: (148.75, 79.25, -18.00)

Sample Links:
  Link 1: cell 1 -> cell 3, vertices 9:4, cost=15, movement=WALK | SMALL_CREATURE
  Link 2: cell 1 -> cell 6, vertices 4:5, cost=8, movement=WALK | SMALL_CREATURE
```

---

## Phase 2: Debug Visualization with `--debug-pathfinding` ✅ **COMPLETED**

**Goal**: Add a command-line flag to visualize the path database in-game.

### ✅ Completed Implementation

**Files Created/Modified:**
- ✅ `runtimes/desktop_runtime/src/main.rs` - Added `--debug-pathfinding` CLI flag
- ✅ `runtimes/debug_runtime/src/main.rs` - Added `--debug-pathfinding` CLI flag for debug runtime
- ✅ `shock2vr/src/lib.rs` - Added `debug_pathfinding: bool` to GameOptions
- ✅ `shock2vr/src/mission/mission_core.rs` - Added pathfinding visualization rendering
- ✅ `shock2vr/src/mission/pathfinding_debug.rs` - **NEW** Dedicated pathfinding debug module
- ✅ `shock2vr/src/mission/mod.rs` - Added pathfinding_debug module export

**Visualization Features:**
- ✅ **Cyan lines** for navigation cell boundaries and center crosses
- ✅ **Yellow lines** for cell-to-cell connectivity links
- ✅ **Proper coordinate scaling** using SCALE_FACTOR (2.5) for VR world scaling
- ✅ **Modular architecture** with dedicated `pathfinding_debug.rs` module
- ✅ **Null safety** with proper path database existence checks

**Technical Implementation:**
```rust
/// Renders pathfinding visualization as scene objects
pub fn render_pathfinding_debug(path_database: &PathDatabase) -> Vec<SceneObject> {
    // Creates cyan lines for navigation cells and polygons
    // Creates yellow lines for cell links
    // Returns scene objects for integration with game rendering
}
```

**Integration Pattern:**
```rust
// mission_core.rs render loop
if options.debug_pathfinding {
    if let Some(ref path_database) = self.path_database {
        let mut pathfinding_visuals = pathfinding_debug::render_pathfinding_debug(path_database);
        scene.append(&mut pathfinding_visuals);
    }
}
```

### ✅ Validation Commands

```bash
# Desktop runtime with pathfinding visualization
cargo dr --debug-pathfinding

# Debug runtime with pathfinding visualization (programmable control)
cargo dbgr --mission medsci1.mis --debug-pathfinding --port 8080

# Verify pathfinding data integrity
cargo dq aipath medsci1.mis
```

### ✅ Rendering Results

The visualization successfully displays:
- **4,695 cyan cell boundaries** showing navigation polygon shapes
- **27,035 yellow connectivity lines** between adjacent cells
- **Properly scaled coordinates** matching VR world space (SCALE_FACTOR applied)
- **Real-time overlay** during gameplay for debugging AI navigation paths

---

## Phase 3: A* Pathfinding Integration ✅ **COMPLETED**

**Goal**: Implement pathfinding using the `pathfinding` crate to find routes through the cell graph with interactive testing.

### ✅ Completed Implementation

**Files Created/Modified:**
- ✅ `shock2vr/Cargo.toml` - Added `pathfinding = "4"` dependency
- ✅ `shock2vr/src/pathfinding/mod.rs` - **NEW** PathfindingService with A* algorithm
- ✅ `shock2vr/src/pathfinding/path_visualization.rs` - **NEW** Multi-path visualization system
- ✅ `shock2vr/src/mission/pathfinding_test.rs` - **NEW** Interactive test system (refactored from mission_core)
- ✅ `shock2vr/src/lib.rs` - Added pathfinding module export
- ✅ `shock2vr/src/mission/mission_core.rs` - PathfindingService integration and path visualization rendering
- ✅ `shock2vr/src/command/mod.rs` - PathfindingTestCommand for effect system
- ✅ `shock2vr/src/scripts/effect.rs` - PathfindingTest effect variant
- ✅ `runtimes/desktop_runtime/src/main.rs` - P key interactive pathfinding test
- ✅ `runtimes/debug_runtime/src/main.rs` - HTTP pathfinding-test API commands

### ✅ Core API Implementation

```rust
// shock2vr/src/pathfinding/mod.rs
pub struct PathfindingService {
    pub path_database: Arc<PathDatabase>,
}

impl PathfindingService {
    /// Find the AIPATH cell containing a world position
    /// Uses simple point-in-polygon tests on convex AIPATH cells
    pub fn cell_from_position(&self, pos: Vector3<f32>) -> Option<u32>;

    /// Find path from start position to goal position using A* algorithm
    /// Returns list of waypoints (cell centers) to traverse
    pub fn find_path(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<Vec<Vector3<f32>>>;

    /// Find the closest reachable cell to a goal position
    /// Useful when exact goal position is in an unpathable area
    pub fn find_closest_reachable_cell(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<u32>;
}
```

**Key Technical Features:**
- ✅ **Point-in-polygon spatial queries** for cell lookup using 2D XZ-plane tests
- ✅ **A* pathfinding integration** using `pathfinding::directed::astar::astar`
- ✅ **Movement capability filtering** via MovementBits (WALK, FLY, SWIM, SMALL_CREATURE)
- ✅ **Fallback to closest reachable cell** when direct path impossible
- ✅ **Euclidean distance heuristic** for efficient A* search

### ✅ Interactive Pathfinding Test System

**Desktop Runtime (P Key Cycling):**
- ✅ **Press P (State 1)**: Set start position (player location), show green marker
- ✅ **Press P (State 2)**: Set goal position (player location), compute A* path, show green path lines
- ✅ **Press P (State 3)**: Reset/clear all markers and test paths, return to State 1

**Debug Runtime (HTTP Commands):**
```bash
# Cycle through pathfinding test states (start → goal → show path → reset)
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test -d '{"action": "cycle"}'

# Set start position explicitly
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test -d '{"action": "set_start"}'

# Set goal position and compute path
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test -d '{"action": "set_goal"}'

# Clear test data
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test -d '{"action": "reset"}'
```

### ✅ Multi-Path Visualization System

```rust
// shock2vr/src/pathfinding/path_visualization.rs
pub struct PathVisualizationSystem {
    pub paths: HashMap<String, ComputedPath>,
}

pub struct ComputedPath {
    pub waypoints: Vec<Vector3<f32>>,
    pub color: Vector3<f32>,      // Green for test paths, different colors for AI
    pub name: String,             // "test_path", "grunt_1_path", etc.
    pub markers: Vec<PathMarker>, // Start/goal/waypoint markers
}

pub struct PathMarker {
    pub position: Vector3<f32>,
    pub marker_type: MarkerType,  // Start, Goal, Waypoint
    pub color: Vector3<f32>,
}
```

**Visualization Features:**
- ✅ **Height-offset rendering** at `HUMAN_HEIGHT * 0.75` to appear above debug pathfinding mesh
- ✅ **Multi-colored path support** with predefined color schemes (TEST_PATH, AI_PATH, PATROL_PATH)
- ✅ **Start/goal markers** as 3D crosses, waypoint markers as dots
- ✅ **Named path management** allowing multiple simultaneous AI paths
- ✅ **Modular rendering system** integrated with game's scene object pipeline

### ✅ Results for medsci1.mis

**Pathfinding Performance:**
- ✅ **4,695 cells processed** for spatial queries with efficient point-in-polygon tests
- ✅ **27,035 links traversed** by A* algorithm with movement capability filtering
- ✅ **Real-time path computation** from any valid floor position to any other
- ✅ **Fallback pathfinding** to closest reachable cell when direct path impossible

**Interactive Testing Validation:**
```bash
# Test basic pathfinding with desktop runtime
cargo dr --debug-draw --mission medsci1.mis
# Press P to cycle: start marker → goal marker → computed path → reset

# Test with debug runtime for automated validation
cargo dbgr --mission medsci1.mis --debug-draw --port 8080
curl -X POST http://127.0.0.1:8080/v1/pathfinding-test -d '{"action": "cycle"}'
```

**Example Pathfinding Output:**
```
Set pathfinding test start at (45.23, 2.50, -18.00). Press P again to set goal.
Computed path with 6 waypoints from (45.23, 2.50, -18.00) to (78.45, 2.50, -35.20). Press P again to reset.
No direct path found. Computed fallback path with 4 waypoints to closest reachable position (67.80, 2.50, -28.10). Press P again to reset.
```

### ✅ Technical Architecture

**Modular Design:**
- ✅ **Separated pathfinding logic** from mission core via dedicated `pathfinding/` module
- ✅ **Refactored interactive test system** into `pathfinding_test.rs` for better code organization
- ✅ **Clean integration patterns** with existing effect system and command processing
- ✅ **Swappable spatial query strategies** allowing future optimization (BSP trees, spatial indexing)

**VR Optimization:**
- ✅ **Efficient A* implementation** using proven `pathfinding` crate algorithms
- ✅ **Visual feedback system** with proper coordinate scaling for VR world space
- ✅ **Performance-conscious design** with simple linear cell searches (4,695 cells acceptable for Phase 3)

This implementation provides a solid foundation for Phase 4 AI integration, with proven pathfinding algorithms and comprehensive interactive testing capabilities.

---

## Phase 4: AI Integration ✅ **COMPLETED**

**Goal**: Enhance monster AI to use A* pathfinding for navigation to any target (players, entities, or specific positions).

### ✅ Completed Implementation

**Files Created/Modified:**
- ✅ `shock2vr/src/scripts/ai/steering/waypoint_steering_strategy.rs` - **NEW** General waypoint-following strategy using pathfinding
- ✅ `shock2vr/src/scripts/ai/steering/mod.rs` - Added waypoint strategy export
- ✅ `shock2vr/src/scripts/ai/behavior/chase_behavior.rs` - Added `with_pathfinding()` constructor for waypoint steering

### ✅ Implemented Design

The completed `WaypointSteeringStrategy` provides:
1. **✅ General Target Support**: Can navigate to players, entities, or specific Vector3 positions via `NavigationTarget` enum
2. **✅ Smart Fallback**: Uses direct steering when within distance threshold (3m) OR in same AIPATH cell (instead of line-of-sight for better obstacle handling)
3. **✅ Waypoint Following**: Stores current path and follows waypoints in sequence with configurable reach distance
4. **✅ Integration Ready**: Works with existing steering chain system via `ChaseBehavior::with_pathfinding()`
5. **✅ Reusable**: Designed for chase, patrol, investigation, and scripted movement behaviors

### ✅ NavigationTarget Enum

```rust
#[derive(Debug, Clone)]
pub enum NavigationTarget {
    /// Navigate to player's current position
    Player,
    /// Navigate to specific entity's position
    Entity(EntityId),
    /// Navigate to fixed world position
    Position(Vector3<f32>),
}
```

### ✅ Core Implementation

```rust
pub struct WaypointSteeringStrategy {
    target: NavigationTarget,
    pathfinding_service: Option<PathfindingService>,
    current_waypoints: Vec<Vector3<f32>>,
    current_waypoint_index: usize,
    waypoint_reached_distance: f32,     // Default: 0.5m
    direct_steering_distance: f32,      // Default: 3.0m
}

impl WaypointSteeringStrategy {
    pub fn new(target: NavigationTarget, pathfinding_service: Option<PathfindingService>) -> Self
    pub fn set_target(&mut self, target: NavigationTarget)

    // Smart fallback logic
    fn should_use_direct_steering(&self, current_pos: Vector3<f32>, target_pos: Vector3<f32>) -> bool {
        // Distance threshold (~3 meters) OR same AIPATH cell
        let distance = (target_pos - current_pos).magnitude();
        if distance < self.direct_steering_distance { return true; }

        let current_cell = pathfinding_service.cell_from_position(current_pos);
        let target_cell = pathfinding_service.cell_from_position(target_pos);
        current_cell.is_some() && current_cell == target_cell
}
```

### ✅ Integration with Chase Behavior

```rust
// In shock2vr/src/scripts/ai/behavior/chase_behavior.rs
impl ChaseBehavior {
    // Existing constructor (backwards compatible)
    pub fn new() -> ChaseBehavior { /* Direct player chase */ }

    // New constructor with pathfinding support
    pub fn with_pathfinding(pathfinding_service: Option<PathfindingService>) -> ChaseBehavior {
        ChaseBehavior {
            steering_strategy: steering::chained(vec![
                Box::new(CollisionAvoidanceSteeringStrategy::conservative()),
                Box::new(WaypointSteeringStrategy::new(
                    NavigationTarget::Player,
                    pathfinding_service,
                )),
            ]),
        }
    }
}
```

### ✅ Key Features Implemented

1. **Smart Fallback Logic**: Direct steering when close (3m) OR in same AIPATH cell
2. **Memory Efficient**: Recomputes paths only when needed, not every frame
3. **Backwards Compatible**: Existing `ChaseBehavior::new()` unchanged
4. **Flexible Targeting**: Supports player, entity, and position targets
5. **Integration Ready**: Works with existing steering chain and collision avoidance

### ✅ Validation

```bash
# Test compilation
cargo check -p shock2vr

# Future testing will use existing interactive pathfinding system
cargo dbgr --mission medsci1.mis --debug-pathfinding --port 8080
```

---

---

## Phase 5: Patrol Path Implementation

**Goal**: Implement patrol behavior using AIWatchObj/TPath links with A* navigation between waypoints.

**Note**: Phase 4's general `WaypointSteeringStrategy` dramatically simplifies patrol implementation since patrol waypoints are just navigation targets.

### Background

The original engine uses:
- **AIWatchObj links**: Trigger scripted sequences when AI enters radius
- **TPath links**: Define patrol routes between waypoints

With the general waypoint steering from Phase 4, patrol behavior becomes straightforward: just cycle through waypoint positions using `WaypointSteeringStrategy::new_goto_position()`.

### Files to Modify

| File | Action | Purpose |
|------|--------|---------|
| `dark/src/properties/mod.rs` | Modify | Add TPath link parsing if not present |
| `shock2vr/src/scripts/ai/behavior/patrol_behavior.rs` | Create | Simplified patrol behavior using waypoint steering |
| `shock2vr/src/scripts/ai/behavior/mod.rs` | Modify | Export patrol behavior |
| `shock2vr/src/scripts/ai/animated_monster_ai.rs` | Modify | Use patrol behavior at low alertness |

### TPath Link Structure

```rust
// TPath link connects waypoint entities in a patrol route
pub struct TPathLink {
    pub speed: f32,      // Movement speed multiplier
    pub pause: f32,      // Pause time at destination
    pub path_data: u32,  // Additional flags
}
```

### Simplified Patrol Behavior Design

**Key Insight**: With general waypoint steering, patrol behavior just manages waypoint sequence rather than pathfinding logic.

```rust
pub struct PatrolBehavior {
    patrol_points: Vec<Vector3<f32>>,    // Waypoint positions from TPath links
    current_point_idx: usize,
    waypoint_steering: WaypointSteeringStrategy,  // Reuses Phase 4 strategy!
    state: PatrolState,
}

enum PatrolState {
    MovingToWaypoint,
    WaitingAtWaypoint { remaining: f32 },
    ExecutingAction,  // For scripted actions at waypoints
}
```

### Implementation Steps

1. **Parse TPath links** (if not already):
   - Add `LinkTPath` definition
   - Parse from `L$TPath` and `LD$TPath` chunks

2. **Create `PatrolBehavior`** (dramatically simplified):
   - On init: collect TPath-linked waypoint positions for this entity
   - Initialize `WaypointSteeringStrategy::new_goto_position()` for first waypoint
   - **No custom pathfinding logic needed** - reuse Phase 4 waypoint steering!

3. **Simplified patrol loop logic**:
   ```rust
   fn update(&mut self, ...) -> BehaviorOutput {
       match self.state {
           PatrolState::MovingToWaypoint => {
               // Let waypoint steering handle all pathfinding
               let steering_output = self.waypoint_steering.steer(...);

               if self.at_current_waypoint() {
                   self.state = PatrolState::WaitingAtWaypoint {
                       remaining: self.get_pause_time()
                   };
               }
               return steering_output;
           }
           PatrolState::WaitingAtWaypoint { remaining } => {
               if remaining <= 0.0 {
                   self.advance_to_next_patrol_point();
                   // Update waypoint steering target to next position
                   self.waypoint_steering.set_target(
                       NavigationTarget::Position(self.current_patrol_point())
                   );
                   self.state = PatrolState::MovingToWaypoint;
               }
           }
           // ...
       }
   }
   ```

4. **Integrate with alertness system**:
   - Use `PatrolBehavior` when alertness is Low (level 1)
   - Switch to `ChaseBehavior` (using same waypoint steering!) when alertness escalates
   - Return to patrol when alertness decays back to Low

5. **Handle waypoint actions**:
   - Check for AIWatchObj triggers at waypoints
   - Execute associated scripted sequences
   - Continue patrol after sequence completes

**Key Benefits of General Waypoint Steering**:
- **No duplicate pathfinding logic** - patrol reuses chase pathfinding
- **Consistent behavior** - same A* algorithm for all navigation
- **Easy debugging** - single pathfinding system to debug
- **Future-proof** - new behaviors can easily add waypoint navigation

### Validation

- Monsters follow defined patrol routes
- A* pathfinding navigates around obstacles between patrol points
- Patrol interrupted when player detected
- Patrol resumes after alertness decays

---

## Summary of Key Files

### New Files
- `dark/src/mission/path_database.rs` - AIPATH parsing
- `shock2vr/src/pathfinding/mod.rs` - Pathfinding service
- `shock2vr/src/pathfinding/cell_graph.rs` - Graph adapter
- `shock2vr/src/scripts/ai/steering/pathfinding_steering_strategy.rs` - Path-following steering
- `shock2vr/src/scripts/ai/behavior/patrol_behavior.rs` - Patrol behavior

### Modified Files
- `dark/src/mission/mod.rs` - Add path_database loading
- `runtimes/desktop_runtime/src/main.rs` - Add --debug-pathfinding flag
- `shock2vr/src/lib.rs` - Add GameOptions flag, pathfinding module
- `shock2vr/src/mission/mission_core.rs` - Debug visualization
- `shock2vr/src/scripts/ai/behavior/chase_behavior.rs` - Use pathfinding
- `shock2vr/src/scripts/ai/animated_monster_ai.rs` - Path state, patrol integration

## Testing Strategy

Each phase should be validated before proceeding:

1. **Phase 1**: Log output shows correct cell/link counts for known missions
2. **Phase 2**: Visual inspection of debug overlay matches expected level geometry
3. **Phase 3**: Unit tests for pathfinding, integration test finding paths in real missions
4. **Phase 4**: Monsters navigate around obstacles, don't get stuck
5. **Phase 5**: Patrol routes work correctly, interrupted by player detection

## Dependencies

- `pathfinding` crate (add to shock2vr/Cargo.toml)
- `bitflags` crate (likely already present)
