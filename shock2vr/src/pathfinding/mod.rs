/// Pathfinding module for AI navigation using AIPATH data
///
/// This module provides A* pathfinding capabilities using the navigation mesh
/// stored in AIPATH chunks. It maintains separation from the BSP tree system
/// used for rendering/visibility queries.
pub mod async_queries;
pub mod path_visualization;

use cgmath::{InnerSpace, Vector3};
use dark::{
    SCALE_FACTOR,
    mission::{
        PathDatabase,
        path_database::{MovementBits, NO_ZONE, PathCell, PathCellFlags, PathCellLink},
    },
};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Taut crossing points keep this distance (3 Dark feet) from shared-edge
/// endpoints. Endpoints are wall corners: a fully-taut path has zero
/// clearance there, dragging the AI's body through the corner. 1.5 feet
/// proved insufficient - it is under the widest creature capsule radius, so
/// taut routes grazed door frames and the body WEDGED on the corner pushing
/// full speed into it forever (stall cleared the path, the re-path produced
/// the identical taut route - issue #481's remaining freeze). One fixed
/// radius for all creatures for now - it also (harmlessly) insets interior
/// subdivision edges, not just walls.
const EDGE_CLEARANCE: f32 = 3.0 / SCALE_FACTOR;

/// Width of the body a walking AI has to fit through (2.4 Dark feet - the
/// widest humanoid capsule, hybrids/grunts at 0.48 world-unit radius). The
/// original engine baked clearance offline and deleted portals a creature
/// could not pass; shipped okBits carry no such marking for the sliver
/// portals we synthesize walkability for, so we gate them geometrically.
const AGENT_WIDTH: f32 = 2.4 / SCALE_FACTOR;

/// Extra cost (Dark feet of detour) for crossing a portal narrower than the
/// walker's body. Large enough to outweigh any local detour around the
/// pinch, small enough that a sliver still beats no route at all - the
/// shipped mesh sometimes has no other way into a spot.
const NARROW_PORTAL_PENALTY: u32 = 250;

/// A zero-bit link counts as a flat walkable seam when the two cell floors
/// differ by no more than this (3 Dark feet - covers small steps; genuine
/// cliffs in the data differ by up to 15)
const FLAT_SEAM_MAX_CENTER_DY: f32 = 3.0 / SCALE_FACTOR;
/// ...and its shared-edge vertices sit within this of both floors
const FLAT_SEAM_MAX_EDGE_DY: f32 = 5.0 / SCALE_FACTOR;

/// Island bridging (experimental `nav_bridges`): maximum vertex-to-vertex
/// gap between two islands' cells to synthesize a crossing link (10 Dark
/// feet - wide enough for stair strips and doorway thresholds that shipped
/// with no nav cells)
const BRIDGE_MAX_GAP: f32 = 10.0 / SCALE_FACTOR;
/// Spatial-hash bucket size for bridge candidate search (Dark feet)
const BRIDGE_BUCKET: f32 = 12.0 / SCALE_FACTOR;
/// Cost multiplier for entering a blocking-OBB cell in relaxed navigation:
/// prefer clear floor, but allow squeezing past baked furniture
const BLOCKING_CELL_COST_PENALTY: u32 = 4;

/// How long a steering-reported blocked crossing stays excluded from an
/// AI's path queries. Long enough that the re-path (and several after it)
/// route around the obstacle instead of reproducing the blocked route;
/// short enough that a transient blocker (a shoved crate) is retried
/// within a minute.
const BLOCKED_LINK_TTL_SECONDS: f32 = 30.0;
/// Cap on remembered blocked crossings (new reports dropped while full of
/// live entries) - bounds memory; entries expire on their TTL.
const MAX_BLOCKED_LINKS: usize = 64;

/// Per-frame cap on AI-initiated pathfind queries. A slot now covers the
/// full fallback chain (A* + stressed retry + partial-route Dijkstra when
/// the goal is unreachable), benched at ~0.5ms p95 / ~3ms worst per slot on
/// desktop. Quest's CPU is several times slower against a ~13.9ms frame at
/// 72Hz, so Android gets one slot per frame; AIs that miss a slot keep
/// steering along their stale path and re-path on a later frame.
#[cfg(target_os = "android")]
pub const PATHFINDING_QUERIES_PER_FRAME: u32 = 1;
#[cfg(not(target_os = "android"))]
pub const PATHFINDING_QUERIES_PER_FRAME: u32 = 2;

/// Shipyard unique holding the remaining AI pathfind-query budget for the
/// current frame. Reset by the mission update each frame; path-following
/// steering acquires a slot before re-pathing and defers when exhausted, so
/// N simultaneously-alerted AIs can't stack N searches into one frame.
/// Atomic so it works behind a shared (UniqueView) borrow.
#[derive(shipyard::Unique)]
pub struct PathfindingFrameBudget(AtomicU32);

impl PathfindingFrameBudget {
    pub fn new() -> Self {
        Self(AtomicU32::new(PATHFINDING_QUERIES_PER_FRAME))
    }

    /// Refill the budget (call once at the start of each frame)
    pub fn reset(&self) {
        self.0
            .store(PATHFINDING_QUERIES_PER_FRAME, Ordering::Relaxed);
    }

    /// Take one query slot. Returns false when the frame's budget is
    /// exhausted - the caller should defer to a later frame.
    pub fn try_acquire(&self) -> bool {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_sub(1))
            .is_ok()
    }
}

impl Default for PathfindingFrameBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// Monotonic query counters, for load measurement and budget verification.
/// Callers diff snapshots across frames to get rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathfindingStats {
    /// Total find_path calls
    pub queries: u64,
    /// Queries that ran the stressed second pass (first pass failed)
    pub stressed_retries: u64,
    /// Queries that found no route at all (the most expensive outcome)
    pub no_route: u64,
}

/// Outcome of an AI's most recent path query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPathOutcome {
    /// A* reached the goal
    Full,
    /// Goal unreachable; routed to the closest reachable point instead
    Partial,
    /// No route at all (not even partial progress)
    Failed,
}

/// The most recent path an AI computed, for debug introspection
/// (GET /v1/ai/paths in the debug runtime)
#[derive(Debug, Clone)]
pub struct AiPathRecord {
    pub goal: Vector3<f32>,
    pub waypoints: Vec<Vector3<f32>>,
    pub outcome: AiPathOutcome,
}

/// Live path-following state, published by the steering every frame it
/// runs - shows what the strategy is ACTUALLY following (which can lag or
/// differ from the latest computed route in `AiPathRecord`)
#[derive(Debug, Clone, Copy)]
pub struct AiSteeringDebug {
    /// Index of the waypoint currently steered toward
    pub next_waypoint: usize,
    /// Length of the path being followed (0 = no active path)
    pub path_len: usize,
    /// World position currently steered toward (waypoint), if any
    pub target: Option<Vector3<f32>>,
    /// Seconds without progress toward the current waypoint
    pub stall_seconds: f32,
}

/// Pathfinding service for AI navigation
///
/// Uses AIPATH cells for navigation mesh queries and A* pathfinding.
/// Keeps the spatial query implementation simple and swappable.
pub struct PathfindingService {
    pub path_database: Arc<PathDatabase>,
    /// Outgoing link indices for each cell, so A* expansion is O(degree)
    /// instead of a scan over every link in the mission. Indices >=
    /// `path_database.links.len()` refer to `bridge_links`.
    links_by_cell: Vec<Vec<u32>>,
    /// Below-door cell -> the mission object id of the door that gates it
    /// (from the AIPATH cell-door table). Lets an AI look up which door
    /// blocks a cell on its route.
    cell_to_door: std::collections::HashMap<u32, i32>,
    /// Effective traversal bits per link (db links, then bridge links).
    /// A link's own `ok_bits` byte is not the whole story in v2.9 data:
    /// cross-zone links inherit bits from the zone-pair table, and flat
    /// zero-bit seams (pervasive in shipped data) are granted WALK.
    effective_bits: Vec<MovementBits>,
    /// Free width at each link's portal (distance from the portal to the
    /// nearest wall in the mesh), parallel to `effective_bits`. A portal
    /// with less than the walker's radius is impassable for anything but a
    /// small creature. Synthesized bridge links carry `f32::INFINITY` -
    /// their clearance is validated by raycast when they are built.
    portal_clearances: Vec<f32>,
    /// Free half-width at each cell's center, parallel to
    /// `path_database.cells`: a cell whose center is tighter than the
    /// walker's radius is no place to send a body.
    cell_clearances: Vec<f32>,
    /// Synthesized island-crossing links (experimental `nav_bridges`),
    /// indexed after `path_database.links`
    bridge_links: Vec<PathCellLink>,
    /// Relaxed navigation (experimental `nav_bridges`): blocking-OBB cells
    /// (furniture baked into the mesh at build time) are passable at a cost
    /// penalty - the objects are physically simulated, steering handles them
    relaxed: bool,
    /// Latest path per AI entity (key: EntityId::inner()), recorded by the
    /// path-follow steering so tooling can see what each AI is doing
    ai_paths: std::sync::Mutex<HashMap<u64, AiPathRecord>>,
    /// Live steering state per AI (what it is actually following right now)
    ai_steering: std::sync::Mutex<HashMap<u64, AiSteeringDebug>>,
    /// Cell crossings some AI's steering failed to traverse (a stall fired
    /// mid-route): obstacles the mesh doesn't model, e.g. a physical prop
    /// or a scripted stationary NPC sitting on a walkable link. Entries map
    /// `(from_cell, to_cell) -> expiry in mission seconds`. SHARED across
    /// AIs: the blockage is a physical fact about the world, and per-AI
    /// memory let a capture pocket re-trap every fresh arrival while each
    /// separately re-learned it (measured at medsci1's FemaleMedsci
    /// pocket). All AI path queries skip unexpired crossings, so after the
    /// first capture the re-paths - everyone's - route around the obstacle
    /// (issue #481's grind loop). Directed-link granularity (not whole
    /// cells) so one blocked doorway can't seal every other entrance into
    /// a large cell.
    blocked_links: std::sync::Mutex<HashMap<(u32, u32), f32>>,
    /// Mission object ids of doors that are impassable: locked-and-closed,
    /// permanently closed, or otherwise inoperable. A* refuses links into
    /// their below-door cells; closed-but-openable doors stay pathable and
    /// are opened on arrival.
    /// Synced from live door state by the mission update.
    blocked_doors: std::sync::RwLock<std::collections::HashSet<i32>>,
    queries: AtomicU64,
    stressed_retries: AtomicU64,
    no_route: AtomicU64,
}

impl PathfindingService {
    /// Create a new pathfinding service with the given path database
    /// (faithful traversal rules; no island bridging)
    pub fn new(path_database: Arc<PathDatabase>) -> Self {
        Self::with_nav_options(path_database, false, None)
    }

    /// Create a pathfinding service; `bridge_islands` enables the
    /// experimental mesh reconnection (island-crossing links + relaxed
    /// blocking-cell traversal) for full-map navigation. `nav_validator`
    /// (when provided) vets synthesized island bridges - given two points,
    /// return whether the straight walk between them is physically clear
    /// (doors and creatures should count as clear: doors open at runtime,
    /// creatures wander off). A corpus sweep of all 23 shipped missions
    /// found the SHIPPED graph's own links physically clear everywhere once
    /// unopenable doors are sealed (see the mission update's door sync), so
    /// only synthesized links are validated.
    pub fn with_nav_options(
        path_database: Arc<PathDatabase>,
        bridge_islands: bool,
        nav_validator: Option<&dyn Fn(Vector3<f32>, Vector3<f32>) -> bool>,
    ) -> Self {
        let mut links_by_cell = vec![Vec::new(); path_database.cells.len()];
        for (idx, link) in path_database.links.iter().enumerate() {
            if let Some(links) = links_by_cell.get_mut(link.from_cell as usize) {
                links.push(idx as u32);
            }
        }
        let cell_to_door = path_database
            .cell_doors
            .iter()
            .map(|cd| (cd.cell, cd.door))
            .collect();
        let mut effective_bits = compute_effective_bits(&path_database);
        let boundary = NavBoundary::build(&path_database);
        let mut portal_clearances = boundary.portal_clearances(&path_database);
        let cell_clearances = boundary.cell_clearances(&path_database);
        let bridge_links = if bridge_islands {
            compute_bridge_links(&path_database, &effective_bits, nav_validator)
        } else {
            Vec::new()
        };
        for (offset, link) in bridge_links.iter().enumerate() {
            let idx = (path_database.links.len() + offset) as u32;
            if let Some(links) = links_by_cell.get_mut(link.from_cell as usize) {
                links.push(idx);
            }
            effective_bits.push(link.ok_bits);
            portal_clearances.push(f32::INFINITY);
        }
        if !bridge_links.is_empty() {
            tracing::info!(
                "pathfinding: synthesized {} island-bridge links (nav_bridges)",
                bridge_links.len()
            );
        }
        Self {
            path_database,
            links_by_cell,
            cell_to_door,
            effective_bits,
            portal_clearances,
            cell_clearances,
            bridge_links,
            relaxed: bridge_islands,
            ai_paths: std::sync::Mutex::new(HashMap::new()),
            ai_steering: std::sync::Mutex::new(HashMap::new()),
            blocked_links: std::sync::Mutex::new(HashMap::new()),
            blocked_doors: std::sync::RwLock::new(std::collections::HashSet::new()),
            queries: AtomicU64::new(0),
            stressed_retries: AtomicU64::new(0),
            no_route: AtomicU64::new(0),
        }
    }

    /// Replace the set of impassable doors (mission object ids). Their
    /// below-door cells remain unpathable until the next state sync removes
    /// them.
    pub fn set_blocked_doors(&self, doors: std::collections::HashSet<i32>) {
        if let Ok(mut blocked) = self.blocked_doors.write() {
            *blocked = doors;
        }
    }

    /// Number of synthesized island-bridge links (0 in faithful mode)
    pub fn bridge_link_count(&self) -> usize {
        self.bridge_links.len()
    }

    /// Drop AI path and steering records whose entity no longer satisfies
    /// `keep` (it despawned, or died - a corpse keeps its entity), so
    /// introspection doesn't report ghosts. Blocked crossings are world
    /// facts, not per-entity records - they expire on their own TTL.
    pub fn prune_ai_paths(&self, keep: impl Fn(u64) -> bool) {
        if let Ok(mut paths) = self.ai_paths.lock() {
            paths.retain(|&entity, _| keep(entity));
        }
        if let Ok(mut steering) = self.ai_steering.lock() {
            steering.retain(|&entity, _| keep(entity));
        }
    }

    /// Remember that an AI could not physically traverse the crossing
    /// `from_cell -> to_cell` (its steering stalled mid-route): the
    /// crossing is excluded from ALL AI path queries until the entry
    /// expires, so re-paths route around the obstacle - including for
    /// fresh arrivals that haven't hit it yet.
    pub fn report_blocked_link(&self, from_cell: u32, to_cell: u32, now_seconds: f32) {
        let Ok(mut blocked) = self.blocked_links.lock() else {
            return;
        };
        blocked.retain(|_, &mut expiry| expiry > now_seconds);
        if blocked.len() >= MAX_BLOCKED_LINKS && !blocked.contains_key(&(from_cell, to_cell)) {
            return; // full of live entries - drop rather than grow unbounded
        }
        blocked.insert((from_cell, to_cell), now_seconds + BLOCKED_LINK_TTL_SECONDS);
    }

    /// The `(from_cell, to_cell)` crossings currently excluded from AI path
    /// queries (unexpired steering-reported blockages). Expired entries are
    /// dropped.
    pub fn blocked_links(&self, now_seconds: f32) -> std::collections::HashSet<(u32, u32)> {
        let Ok(mut blocked) = self.blocked_links.lock() else {
            return std::collections::HashSet::new();
        };
        blocked.retain(|_, &mut expiry| expiry > now_seconds);
        blocked.keys().copied().collect()
    }

    /// Record the latest path an AI computed (key: EntityId::inner())
    pub fn record_ai_path(&self, entity: u64, record: AiPathRecord) {
        if let Ok(mut paths) = self.ai_paths.lock() {
            paths.insert(entity, record);
        }
    }

    /// Publish an AI's live steering state (called by path-follow steering
    /// every frame it runs)
    pub fn record_ai_steering(&self, entity: u64, debug: AiSteeringDebug) {
        if let Ok(mut steering) = self.ai_steering.lock() {
            steering.insert(entity, debug);
        }
    }

    /// The live steering state for an entity, if it has published any
    pub fn ai_steering(&self, entity: u64) -> Option<AiSteeringDebug> {
        self.ai_steering
            .lock()
            .ok()
            .and_then(|steering| steering.get(&entity).copied())
    }

    /// Snapshot of every AI's latest recorded path
    pub fn ai_paths(&self) -> Vec<(u64, AiPathRecord)> {
        self.ai_paths
            .lock()
            .map(|paths| paths.iter().map(|(k, v)| (*k, v.clone())).collect())
            .unwrap_or_default()
    }

    /// Look up a link by combined index (db links, then bridge links)
    fn link_at(&self, idx: u32) -> &PathCellLink {
        let idx = idx as usize;
        let n = self.path_database.links.len();
        if idx < n {
            &self.path_database.links[idx]
        } else {
            &self.bridge_links[idx - n]
        }
    }

    /// Doors that gate `cell` itself or any cell directly reachable from it -
    /// i.e. the doors an AI standing in `cell` is about to walk into. Each
    /// entry is `(door object id, the door cell's center)`; deduplicated by
    /// door. A short breadth-first reach lets an AI notice (and open) a door
    /// before it walks its body into the closed leaf and stalls. Empty when
    /// the mission has no door data.
    pub fn doors_near_cell(&self, cell: u32) -> Vec<(i32, Vector3<f32>)> {
        if self.cell_to_door.is_empty() {
            return Vec::new();
        }
        // Breadth-first over the walk graph out to a few hops
        const DOOR_LOOKAHEAD_HOPS: u32 = 3;
        let mut out = Vec::new();
        let mut seen_doors = std::collections::HashSet::new();
        let mut visited = std::collections::HashSet::from([cell]);
        let mut frontier = vec![cell];
        for _ in 0..=DOOR_LOOKAHEAD_HOPS {
            let mut next = Vec::new();
            for c in frontier {
                if let Some(&door) = self.cell_to_door.get(&c) {
                    if seen_doors.insert(door) {
                        if let Some(pc) = self.path_database.cells.get(c as usize) {
                            out.push((door, pc.center));
                        }
                    }
                }
                if let Some(links) = self.links_by_cell.get(c as usize) {
                    for &idx in links {
                        let to = self.link_at(idx).to_cell;
                        if visited.insert(to) {
                            next.push(to);
                        }
                    }
                }
            }
            frontier = next;
        }
        out
    }

    /// Snapshot of the monotonic query counters
    pub fn stats(&self) -> PathfindingStats {
        PathfindingStats {
            queries: self.queries.load(Ordering::Relaxed),
            stressed_retries: self.stressed_retries.load(Ordering::Relaxed),
            no_route: self.no_route.load(Ordering::Relaxed),
        }
    }

    /// Find the AIPATH cell containing a world position
    ///
    /// Uses point-in-polygon tests in the XZ plane on convex AIPATH cells.
    /// Cells from different floors overlap in XZ, so among matches we pick
    /// the cell whose floor height is closest to the query position (the
    /// original engine raycasts to the floor for the same reason -
    /// AIFindClosestCell in aipthloc.cpp).
    pub fn cell_from_position(&self, pos: Vector3<f32>) -> Option<u32> {
        let mut best: Option<(u32, f32)> = None;
        for (idx, cell) in self.path_database.cells.iter().enumerate() {
            if self.point_in_cell(pos, cell) {
                let dy = (pos.y - cell.center.y).abs();
                if best.map(|(_, best_dy)| dy < best_dy).unwrap_or(true) {
                    best = Some((idx as u32, dy));
                }
            }
        }
        best.map(|(idx, _)| idx)
    }

    /// Find path from start position to goal position using A* algorithm
    ///
    /// Returns a list of waypoints to traverse, or None if no path exists.
    /// Waypoints are points on the shared edges between cells (not cell
    /// centers), pulled taut toward the goal, mirroring how the original
    /// engine follows cAIPath edges rather than cell centers.
    pub fn find_path(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<Vec<Vector3<f32>>> {
        self.find_path_avoiding(
            start,
            goal,
            movement_bits,
            &std::collections::HashSet::new(),
        )
    }

    /// `find_path` with a set of directed cell crossings to treat as
    /// impassable - steering-reported blockages (see `report_blocked_link`),
    /// so an AI's re-path after a stall routes around the obstacle it just
    /// hit.
    pub fn find_path_avoiding(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
        avoid: &std::collections::HashSet<(u32, u32)>,
    ) -> Option<Vec<Vector3<f32>>> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        if let Some(path) = self.find_path_with_bits(start, goal, movement_bits, avoid) {
            return Some(path);
        }
        // Second pass: a failed pathfind is retried with the stressed
        // condition added (small creatures excepted), so a calm AI still
        // reaches goals whose only route crosses stressed-gated links.
        if !movement_bits.contains(MovementBits::SMALL_CREATURE)
            && !movement_bits.contains(MovementBits::STRESSED)
        {
            self.stressed_retries.fetch_add(1, Ordering::Relaxed);
            if let Some(path) =
                self.find_path_with_bits(start, goal, movement_bits | MovementBits::STRESSED, avoid)
            {
                return Some(path);
            }
        }
        self.no_route.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Find a path that gets as close to `goal` as the mesh allows, for
    /// goals with no full route (different island, off-mesh, behind missing
    /// nav data). Explores everything reachable from `start` and routes to
    /// the reachable cell nearest the goal - the counterpart of the original
    /// engine's pathfind-near facility. Returns None when we're already in
    /// the closest reachable cell (no progress possible).
    pub fn find_path_toward(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<Vec<Vector3<f32>>> {
        self.find_path_toward_avoiding(
            start,
            goal,
            movement_bits,
            &std::collections::HashSet::new(),
        )
    }

    /// `find_path_toward` with steering-reported blocked crossings excluded
    /// (see `find_path_avoiding`) - the partial route then ends BEFORE the
    /// obstacle instead of running through it.
    pub fn find_path_toward_avoiding(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
        avoid: &std::collections::HashSet<(u32, u32)>,
    ) -> Option<Vec<Vector3<f32>>> {
        let start_cell = self.cell_from_position(start)?;
        let reachable = pathfinding::directed::dijkstra::dijkstra_all(&start_cell, |&cell| {
            self.get_successors(cell, movement_bits, avoid)
        });

        let distance_to_goal = |cell: u32| -> f32 {
            (goal - self.path_database.cells[cell as usize].center).magnitude()
        };
        // Stop somewhere the body fits. A partial route is aimed at a cell
        // center, and the cells nearest an unreachable goal are often the
        // pinch it is unreachable through - the wedge at the bottom of a
        // converging corner, where an AI parks itself against the geometry
        // and grinds. Cells too tight to stand in are only considered when
        // nothing roomier is reachable.
        let roomy = |cell: u32| -> bool {
            self.cell_clearances
                .get(cell as usize)
                .is_none_or(|&clearance| clearance >= AGENT_WIDTH * 0.5)
        };
        let mut best = start_cell;
        let mut best_distance = distance_to_goal(start_cell);
        for &cell in reachable.keys() {
            let d = distance_to_goal(cell);
            if d < best_distance && roomy(cell) {
                best_distance = d;
                best = cell;
            }
        }
        if best == start_cell {
            for &cell in reachable.keys() {
                let d = distance_to_goal(cell);
                if d < best_distance {
                    best_distance = d;
                    best = cell;
                }
            }
        }
        if best == start_cell {
            return None;
        }

        // Reconstruct the cell path from the dijkstra parent map
        let mut cell_path = vec![best];
        let mut cursor = best;
        while cursor != start_cell {
            cursor = reachable.get(&cursor)?.0;
            cell_path.push(cursor);
        }
        cell_path.reverse();

        let end = self.path_database.cells[best as usize].center;
        Some(self.waypoints_for_cell_path(&cell_path, start, end, movement_bits))
    }

    fn find_path_with_bits(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
        avoid: &std::collections::HashSet<(u32, u32)>,
    ) -> Option<Vec<Vector3<f32>>> {
        // Find start and goal cells
        let start_cell_id = self.cell_from_position(start)?;
        let goal_cell_id = self.cell_from_position(goal)?;

        // Use pathfinding crate for A* algorithm
        let result = pathfinding::directed::astar::astar(
            &start_cell_id,
            |&cell_id| self.get_successors(cell_id, movement_bits, avoid),
            |&cell_id| self.heuristic(cell_id, goal_cell_id),
            |&cell_id| cell_id == goal_cell_id,
        )?;

        Some(self.waypoints_for_cell_path(&result.0, start, goal, movement_bits))
    }

    /// Convert a cell-id path into world-space waypoints.
    ///
    /// Each cell crossing contributes the point on the shared edge closest to
    /// the next waypoint, computed backward from the goal so the path is
    /// pulled taut instead of zigzagging through cell centers ("diamond"
    /// patterns where many small cells converge at intersections).
    fn waypoints_for_cell_path(
        &self,
        cell_path: &[u32],
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Vec<Vector3<f32>> {
        // Collect the shared edge for each crossing
        let mut edges = Vec::with_capacity(cell_path.len().saturating_sub(1));
        for pair in cell_path.windows(2) {
            let edge = self
                .link_between(pair[0], pair[1], movement_bits)
                .and_then(|link| {
                    let a = *self
                        .path_database
                        .vertices
                        .get(link.edge_vertex_a as usize)?;
                    let b = *self
                        .path_database
                        .vertices
                        .get(link.edge_vertex_b as usize)?;
                    Some(inset_edge(a, b))
                });
            edges.push(edge.unwrap_or_else(|| {
                // No usable edge data; fall back to the destination cell center
                let center = self.path_database.cells[pair[1] as usize].center;
                (center, center)
            }));
        }

        // Pull the path taut: walk the edges backward, aiming each crossing
        // point at the next waypoint (starting from the goal).
        let mut points = vec![Vector3::new(0.0, 0.0, 0.0); edges.len()];
        let mut next_point = goal;
        for (i, (a, b)) in edges.iter().enumerate().rev() {
            let point = closest_point_on_segment(*a, *b, next_point);
            points[i] = point;
            next_point = point;
        }

        let mut waypoints = Vec::with_capacity(edges.len() + 2);
        waypoints.push(start);
        waypoints.extend(points);
        waypoints.push(goal);
        waypoints
    }

    /// Free width the mesh leaves at the portal between two cells (nearest
    /// wall distance at the roomiest point of the shared edge). Inspection
    /// aid for nav tooling; `None` when the cells share no link.
    pub fn portal_clearance(&self, from_cell: u32, to_cell: u32) -> Option<f32> {
        let idx = *self
            .links_by_cell
            .get(from_cell as usize)?
            .iter()
            .find(|&&idx| self.link_at(idx).to_cell == to_cell)?;
        self.portal_clearances.get(idx as usize).copied()
    }

    /// Find a traversable link from one cell to an adjacent cell
    fn link_between(
        &self,
        from_cell: u32,
        to_cell: u32,
        movement_bits: MovementBits,
    ) -> Option<&PathCellLink> {
        self.links_by_cell
            .get(from_cell as usize)?
            .iter()
            .copied()
            .find(|&idx| {
                self.link_at(idx).to_cell == to_cell && self.can_use_link(idx, movement_bits)
            })
            .map(|idx| self.link_at(idx))
    }

    /// Find the closest reachable cell to a goal position
    ///
    /// Useful when the exact goal position is in an unpathable area.
    /// Returns the cell ID of the closest reachable cell.
    pub fn find_closest_reachable_cell(
        &self,
        start: Vector3<f32>,
        goal: Vector3<f32>,
        movement_bits: MovementBits,
    ) -> Option<u32> {
        let start_cell_id = self.cell_from_position(start)?;

        // Find all reachable cells using Dijkstra's algorithm
        let reachable = pathfinding::directed::dijkstra::dijkstra_all(&start_cell_id, |&cell_id| {
            self.get_successors(cell_id, movement_bits, &std::collections::HashSet::new())
        });

        // Find the reachable cell closest to the goal
        let mut closest_cell = Some(start_cell_id); // Start with start cell as fallback
        let start_center = self.path_database.cells[start_cell_id as usize].center;
        let mut closest_distance = (goal - start_center).magnitude();

        // Check all other reachable cells
        for (cell_id, _) in reachable {
            let cell_center = self.path_database.cells[cell_id as usize].center;
            let distance = (goal - cell_center).magnitude();

            if distance < closest_distance {
                closest_distance = distance;
                closest_cell = Some(cell_id);
            }
        }

        closest_cell
    }

    /// Port of the original engine's per-link traversal check: a link is
    /// traversable when the destination cell is not blocked, any condition
    /// bits on the link (stressed / high-strike) are satisfied by the AI,
    /// and the movement medium matches. Uses the link's *effective* bits
    /// (own okBits + zone-pair grants + flat-seam repair), not the raw byte.
    /// Door and app-callback gating are not modeled yet. In relaxed mode
    /// (`nav_bridges`) blocking-OBB cells are passable (penalized in cost).
    fn can_use_link(&self, link_idx: u32, movement_bits: MovementBits) -> bool {
        let link = self.link_at(link_idx);
        let bits = self.effective_bits[link_idx as usize];
        let dest = match self.path_database.cells.get(link.to_cell as usize) {
            Some(dest) => dest,
            None => return false,
        };
        let blocked = if self.relaxed {
            PathCellFlags::UNPATHABLE
        } else {
            PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB
        };
        if dest.flags.intersects(blocked) {
            return false;
        }

        // Door gate: a below-door cell whose door is impassable is not
        // traversable - the AI can't follow the player through it.
        // Closed-but-openable doors stay pathable; the pursuing AI opens them
        // on arrival.
        if dest.flags.contains(PathCellFlags::BELOW_DOOR) {
            if let Some(door) = self.cell_to_door.get(&link.to_cell) {
                if self
                    .blocked_doors
                    .read()
                    .map(|blocked| blocked.contains(door))
                    .unwrap_or(false)
                {
                    return false;
                }
            }
        }

        // Small creatures may only use small-creature links
        if movement_bits.contains(MovementBits::SMALL_CREATURE)
            && !bits.contains(MovementBits::SMALL_CREATURE)
        {
            return false;
        }

        // Condition-gated links (stressed / high-strike) require the AI to be
        // in that condition
        let link_conditions = bits & MovementBits::CONDITION_MASK;
        if !link_conditions.is_empty() && (link_conditions & movement_bits).is_empty() {
            return false;
        }

        // Movement medium must match (condition bits alone don't qualify)
        bits.intersects(movement_bits & !MovementBits::CONDITION_MASK)
    }

    /// Get the successors of a cell for A* pathfinding
    ///
    /// Returns a list of (target_cell_id, cost) pairs for cells reachable
    /// from the given cell. `avoid` holds steering-reported blocked
    /// crossings (physical obstacles the mesh doesn't model) - those
    /// directed links are skipped.
    fn get_successors(
        &self,
        cell_id: u32,
        movement_bits: MovementBits,
        avoid: &std::collections::HashSet<(u32, u32)>,
    ) -> Vec<(u32, u32)> {
        let Some(link_indices) = self.links_by_cell.get(cell_id as usize) else {
            return Vec::new();
        };
        link_indices
            .iter()
            .filter(|&&idx| {
                !avoid.contains(&(cell_id, self.link_at(idx).to_cell))
                    && self.can_use_link(idx, movement_bits)
            })
            .map(|&idx| {
                let link = self.link_at(idx);
                let mut cost = (link.cost as u32).max(1);
                // A portal the body does not fit through costs a detour's
                // worth extra, so A* threads a sliver only when nothing else
                // reaches the goal at all (the mesh's own boundary is the
                // measure - see `portal_clearances`).
                if !movement_bits.contains(MovementBits::SMALL_CREATURE)
                    && self.portal_clearances[idx as usize] < AGENT_WIDTH * 0.5
                {
                    cost += NARROW_PORTAL_PENALTY;
                }
                if self.relaxed {
                    let dest = &self.path_database.cells[link.to_cell as usize];
                    if dest.flags.contains(PathCellFlags::BLOCKING_OBB) {
                        cost *= BLOCKING_CELL_COST_PENALTY;
                    }
                }
                (link.to_cell, cost)
            })
            .collect()
    }

    /// Calculate heuristic distance between two cells for A*
    ///
    /// Link costs are stored in original Dark units (ComputeCell2CellCost in
    /// aipathdb.cpp), while cell centers were divided by SCALE_FACTOR at
    /// parse time - scale back up so the heuristic and the edge costs use
    /// the same units.
    fn heuristic(&self, from_cell: u32, to_cell: u32) -> u32 {
        let from_center = self.path_database.cells[from_cell as usize].center;
        let to_center = self.path_database.cells[to_cell as usize].center;

        ((from_center - to_center).magnitude() * SCALE_FACTOR) as u32
    }

    /// Test if a point is inside a convex AIPATH cell
    ///
    /// Uses simple point-in-polygon test. Since AIPATH cells are convex,
    /// this can be done efficiently by checking that the point is on the
    /// same side of all polygon edges.
    ///
    /// Note: This is a 2D test in the XZ plane, assuming Y coordinate doesn't matter
    /// for floor-based navigation.
    fn point_in_cell(&self, point: Vector3<f32>, cell: &PathCell) -> bool {
        // Skip cells with no vertices
        if cell.vertex_indices.len() < 3 {
            return false;
        }

        // Get the vertices of this cell
        let vertices: Vec<Vector3<f32>> = cell
            .vertex_indices
            .iter()
            .filter_map(|&idx| self.path_database.vertices.get(idx as usize))
            .copied()
            .collect();

        if vertices.len() < 3 {
            return false;
        }

        // Point-in-polygon test using cross products (2D in XZ plane)
        // For a convex polygon, point is inside if it's on the same side of all edges
        let mut sign = None;

        for i in 0..vertices.len() {
            let v1 = vertices[i];
            let v2 = vertices[(i + 1) % vertices.len()];

            // Calculate cross product to determine which side of edge the point is on
            let edge = Vector3::new(v2.x - v1.x, 0.0, v2.z - v1.z);
            let to_point = Vector3::new(point.x - v1.x, 0.0, point.z - v1.z);
            let cross = edge.x * to_point.z - edge.z * to_point.x;

            if cross.abs() < f32::EPSILON {
                continue; // Point is on the edge
            }

            let current_sign = cross > 0.0;

            match sign {
                None => sign = Some(current_sign),
                Some(prev_sign) if prev_sign != current_sign => return false,
                _ => {}
            }
        }

        true
    }
}

/// Effective traversal bits per link. Shipped v2.9 data stores meaningful
/// okBits only on some links; two documented-in-data mechanisms supply the
/// rest:
/// - Cross-zone links inherit the zone-pair reachability table's bits
///   (mission data pairs zones as walkable while the crossing links' own
///   okBits bytes are zero).
/// - Flat zero-bit seams between cells (pervasive between zoned areas and
///   the zone-less connector cells) are granted WALK; the floor-height gates
///   keep genuine cliffs unwalkable.
fn compute_effective_bits(db: &PathDatabase) -> Vec<MovementBits> {
    let zone_of =
        |cell: u32| -> u16 { db.cell_zones.get(cell as usize).copied().unwrap_or(NO_ZONE) };
    let mut pair_bits: HashMap<(u16, u16), MovementBits> = HashMap::new();
    for &(a, b, bits) in &db.zone_pairs {
        *pair_bits.entry((a, b)).or_insert(MovementBits::empty()) |= bits;
        *pair_bits.entry((b, a)).or_insert(MovementBits::empty()) |= bits;
    }

    db.links
        .iter()
        .map(|link| {
            let mut bits = link.ok_bits;
            let (za, zb) = (zone_of(link.from_cell), zone_of(link.to_cell));
            if za != zb && za != NO_ZONE && zb != NO_ZONE {
                if let Some(&granted) = pair_bits.get(&(za, zb)) {
                    bits |= granted;
                }
            }
            if link.ok_bits.is_empty() && is_flat_seam(db, link) {
                bits |= MovementBits::WALK | MovementBits::SMALL_CREATURE;
            }
            bits
        })
        .collect()
}

/// A zero-bit link is a walkable seam when both cell floors and the shared
/// edge sit at (nearly) the same height - rules out the cliff/drop links
/// that also ship with zero okBits.
fn is_flat_seam(db: &PathDatabase, link: &PathCellLink) -> bool {
    let (Some(from), Some(to)) = (
        db.cells.get(link.from_cell as usize),
        db.cells.get(link.to_cell as usize),
    ) else {
        return false;
    };
    if (from.center.y - to.center.y).abs() > FLAT_SEAM_MAX_CENTER_DY {
        return false;
    }
    let (Some(ea), Some(eb)) = (
        db.vertices.get(link.edge_vertex_a as usize),
        db.vertices.get(link.edge_vertex_b as usize),
    ) else {
        return false;
    };
    [ea.y, eb.y].iter().all(|&edge_y| {
        (edge_y - from.center.y).abs() <= FLAT_SEAM_MAX_EDGE_DY
            && (edge_y - to.center.y).abs() <= FLAT_SEAM_MAX_EDGE_DY
    })
}

/// Synthesize island-crossing links (experimental `nav_bridges`).
///
/// The shipped link graph is partitioned into per-area islands with no links
/// between them (stair strips and thresholds have no nav cells at all);
/// original AI pathfinding was area-local. For full-map navigation, connect
/// islands where two cells from different islands come within BRIDGE_MAX_GAP
/// of each other at a walkable slope. Purely geometric - a candidate through
/// thick geometry is possible but rare at these gates, and steering/stall
/// handling copes with the odd bad bridge.
fn compute_bridge_links(
    db: &PathDatabase,
    effective_bits: &[MovementBits],
    validator: Option<&dyn Fn(Vector3<f32>, Vector3<f32>) -> bool>,
) -> Vec<PathCellLink> {
    // Union-find over walk-usable links to label islands
    let mut parent: Vec<u32> = (0..db.cells.len() as u32).collect();
    fn find(parent: &mut Vec<u32>, mut a: u32) -> u32 {
        while parent[a as usize] != a {
            parent[a as usize] = parent[parent[a as usize] as usize];
            a = parent[a as usize];
        }
        a
    }
    let n_cells = db.cells.len() as u32;
    for (idx, link) in db.links.iter().enumerate() {
        if !effective_bits[idx].contains(MovementBits::WALK) {
            continue;
        }
        // Malformed tail links can carry out-of-range cell ids (observed in
        // shipped data, e.g. hydro1); the query paths reject them via
        // checked lookups, and the union must skip them too
        if link.from_cell >= n_cells || link.to_cell >= n_cells {
            continue;
        }
        let (a, b) = (
            find(&mut parent, link.from_cell),
            find(&mut parent, link.to_cell),
        );
        if a != b {
            parent[a as usize] = b;
        }
    }

    // Bucket pathable cells spatially (XZ) for pairwise candidate search
    let mut buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    let bucket_of = |cell: &PathCell| -> (i32, i32) {
        (
            (cell.center.x / BRIDGE_BUCKET).floor() as i32,
            (cell.center.z / BRIDGE_BUCKET).floor() as i32,
        )
    };
    for (idx, cell) in db.cells.iter().enumerate() {
        if cell.flags.contains(PathCellFlags::UNPATHABLE) || cell.vertex_indices.len() < 3 {
            continue;
        }
        buckets.entry(bucket_of(cell)).or_default().push(idx as u32);
    }

    let vertex_gap = |a: &PathCell, b: &PathCell| -> f32 {
        let mut best = f32::INFINITY;
        for &va in &a.vertex_indices {
            for &vb in &b.vertex_indices {
                if let (Some(pa), Some(pb)) =
                    (db.vertices.get(va as usize), db.vertices.get(vb as usize))
                {
                    best = best.min((pa - pb).magnitude());
                }
            }
        }
        best
    };

    // Collect every qualifying candidate first, then union in a stable
    // order (shortest gap first, cell ids as tiebreak) so the synthesized
    // topology is deterministic across launches - HashMap iteration order
    // must not pick the bridges.
    let mut candidates: Vec<(f32, u32, u32)> = Vec::new();
    for (&(bx, bz), cells) in &buckets {
        // Scan the 3x3 bucket neighborhood; a_id < b_id dedupes pairs
        let mut neighborhood: Vec<u32> = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(more) = buckets.get(&(bx + dx, bz + dz)) {
                    neighborhood.extend_from_slice(more);
                }
            }
        }
        for &a_id in cells {
            for &b_id in &neighborhood {
                if a_id >= b_id {
                    continue;
                }
                if find(&mut parent, a_id) == find(&mut parent, b_id) {
                    continue;
                }
                let (a, b) = (&db.cells[a_id as usize], &db.cells[b_id as usize]);
                let dy = (a.center.y - b.center.y).abs();
                let dxz = {
                    let dx = a.center.x - b.center.x;
                    let dz = a.center.z - b.center.z;
                    (dx * dx + dz * dz).sqrt()
                };
                // Slope gate: near-flat for close pairs, up to a stair-like
                // grade across wider gaps
                let max_dy = (2.0 / SCALE_FACTOR).max(1.25 * (dxz - 4.0 / SCALE_FACTOR));
                if dy > max_dy {
                    continue;
                }
                let gap = vertex_gap(a, b);
                if gap > BRIDGE_MAX_GAP {
                    continue;
                }
                candidates.push((gap, a_id, b_id));
            }
        }
    }
    candidates.sort_by(|x, y| {
        x.0.partial_cmp(&y.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(x.1.cmp(&y.1))
            .then(x.2.cmp(&y.2))
    });

    /// Cast height above the cell floors: torso level, clearing steps but
    /// under railings and half-walls a creature can't cross
    const BRIDGE_PROBE_HEIGHT: f32 = 3.5 / SCALE_FACTOR;

    let mut rejected = 0usize;
    let mut bridges = Vec::new();
    for (_, a_id, b_id) in candidates {
        if find(&mut parent, a_id) == find(&mut parent, b_id) {
            continue;
        }
        if let Some(validate) = validator {
            let lift = Vector3::new(0.0, BRIDGE_PROBE_HEIGHT, 0.0);
            let from = db.cells[a_id as usize].center + lift;
            let to = db.cells[b_id as usize].center + lift;
            if !validate(from, to) {
                rejected += 1;
                continue;
            }
        }
        let root_a = find(&mut parent, a_id);
        parent[root_a as usize] = find(&mut parent, b_id);
        let (a, b) = (&db.cells[a_id as usize], &db.cells[b_id as usize]);
        // Cost from center-to-center distance: the executed route runs
        // through the cell centers (no shared edge exists), so this matches
        // travel and keeps the center-distance A* heuristic admissible
        let center_dist = (a.center - b.center).magnitude();
        let cost = ((center_dist * SCALE_FACTOR) as u8).max(1);
        let bits = MovementBits::WALK | MovementBits::SMALL_CREATURE;
        // No shared edge exists; u32::MAX vertex ids make waypoint
        // building fall back to the destination cell center
        for (from, to) in [(a_id, b_id), (b_id, a_id)] {
            bridges.push(PathCellLink {
                from_cell: from,
                to_cell: to,
                edge_vertex_a: u32::MAX,
                edge_vertex_b: u32::MAX,
                ok_bits: bits,
                cost,
            });
        }
    }
    if rejected > 0 {
        tracing::info!(
            "pathfinding: rejected {} island-bridge candidates blocked by geometry",
            rejected
        );
    }
    bridges
}

/// The navigable region's boundary: every stretch of cell edge with no
/// linked, pathable cell on the other side.
///
/// This is what makes a spot too tight for a body. Portal edge lengths do
/// not: the mesh subdivides open floor into many short portals, while the
/// 0.6-wide strip beside a door jamb has portals as long as the strip is
/// wide. Distance to this boundary is the free width, wherever it is
/// measured.
struct NavBoundary {
    /// Wall segments (XZ; `y` from the cell floor they bound)
    walls: Vec<(Vector3<f32>, Vector3<f32>)>,
    /// Wall indices per spatial bucket, for nearest-wall queries
    buckets: HashMap<(i32, i32), Vec<u32>>,
}

/// Vertical window in which a wall counts against a point (5 Dark feet -
/// under a deck height, over any step or railing), so a wall on the deck
/// above doesn't shrink a spot below it
const WALL_FLOOR_SPAN: f32 = 5.0 / SCALE_FACTOR;
/// Spatial-hash bucket for boundary lookup (Dark feet)
const WALL_BUCKET: f32 = 10.0 / SCALE_FACTOR;
/// Collinearity / overlap tolerance when matching one cell's boundary
/// against its neighbours' (world units - shipped cells leave slivers of gap
/// between neighbours, and links name the neighbour's vertices for the same
/// seam, so the shared stretches are matched geometrically rather than by
/// vertex id)
const BOUNDARY_TOLERANCE: f32 = 0.15;

fn bucket_of(x: f32, z: f32) -> (i32, i32) {
    (
        (x / WALL_BUCKET).floor() as i32,
        (z / WALL_BUCKET).floor() as i32,
    )
}

impl NavBoundary {
    fn build(db: &PathDatabase) -> Self {
        let linked: std::collections::HashSet<(u32, u32)> = db
            .links
            .iter()
            .map(|link| (link.from_cell, link.to_cell))
            .collect();

        let mut edges: Vec<(u32, Vector3<f32>, Vector3<f32>)> = Vec::new();
        for cell in &db.cells {
            let n = cell.vertex_indices.len();
            if n < 3 {
                continue;
            }
            for i in 0..n {
                if let (Some(&a), Some(&b)) = (
                    db.vertices.get(cell.vertex_indices[i] as usize),
                    db.vertices.get(cell.vertex_indices[(i + 1) % n] as usize),
                ) {
                    edges.push((cell.id, a, b));
                }
            }
        }
        let mut edge_buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (idx, (_, a, b)) in edges.iter().enumerate() {
            let (bx0, bz0) = bucket_of(a.x.min(b.x), a.z.min(b.z));
            let (bx1, bz1) = bucket_of(a.x.max(b.x), a.z.max(b.z));
            for bx in bx0..=bx1 {
                for bz in bz0..=bz1 {
                    edge_buckets.entry((bx, bz)).or_default().push(idx as u32);
                }
            }
        }

        let blocked = PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB;
        let mut walls: Vec<(Vector3<f32>, Vector3<f32>)> = Vec::new();
        for (edge_idx, &(cell_id, a, b)) in edges.iter().enumerate() {
            let (dx, dz) = (b.x - a.x, b.z - a.z);
            let len = (dx * dx + dz * dz).sqrt();
            if len < BOUNDARY_TOLERANCE {
                continue;
            }
            let (ux, uz) = (dx / len, dz / len);
            // Stretches of this edge shared with a linked, pathable
            // neighbour, as intervals [t0, t1] along it
            let mut open: Vec<(f32, f32)> = Vec::new();
            let (bx0, bz0) = bucket_of(a.x.min(b.x), a.z.min(b.z));
            let (bx1, bz1) = bucket_of(a.x.max(b.x), a.z.max(b.z));
            for bx in (bx0 - 1)..=(bx1 + 1) {
                for bz in (bz0 - 1)..=(bz1 + 1) {
                    let Some(candidates) = edge_buckets.get(&(bx, bz)) else {
                        continue;
                    };
                    for &other in candidates {
                        if other as usize == edge_idx {
                            continue;
                        }
                        let (other_cell, oa, ob) = edges[other as usize];
                        if other_cell == cell_id || !linked.contains(&(cell_id, other_cell)) {
                            continue;
                        }
                        match db.cells.get(other_cell as usize) {
                            Some(cell) if !cell.flags.intersects(blocked) => {}
                            _ => continue,
                        }
                        if (oa.y - a.y).abs() > WALL_FLOOR_SPAN
                            || (ob.y - a.y).abs() > WALL_FLOOR_SPAN
                        {
                            continue;
                        }
                        let on_line = |p: Vector3<f32>| {
                            ((p.x - a.x) * uz - (p.z - a.z) * ux).abs() <= BOUNDARY_TOLERANCE
                        };
                        if !on_line(oa) || !on_line(ob) {
                            continue;
                        }
                        let ta = (oa.x - a.x) * ux + (oa.z - a.z) * uz;
                        let tb = (ob.x - a.x) * ux + (ob.z - a.z) * uz;
                        let (lo, hi) = if ta <= tb { (ta, tb) } else { (tb, ta) };
                        let (lo, hi) = (lo.max(0.0), hi.min(len));
                        if hi - lo > BOUNDARY_TOLERANCE {
                            open.push((lo, hi));
                        }
                    }
                }
            }
            open.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            // What is left of the edge once the shared stretches are removed
            // is wall
            let mut cursor = 0.0f32;
            let point_at = |t: f32| Vector3::new(a.x + ux * t, a.y, a.z + uz * t);
            for (lo, hi) in open {
                if lo - cursor > BOUNDARY_TOLERANCE {
                    walls.push((point_at(cursor), point_at(lo)));
                }
                cursor = cursor.max(hi);
            }
            if len - cursor > BOUNDARY_TOLERANCE {
                walls.push((point_at(cursor), point_at(len)));
            }
        }

        let mut buckets: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (idx, (a, b)) in walls.iter().enumerate() {
            // A wall spans buckets; register it in every one its XZ extent
            // touches so a nearby query can't miss it
            let (bx0, bz0) = bucket_of(a.x.min(b.x), a.z.min(b.z));
            let (bx1, bz1) = bucket_of(a.x.max(b.x), a.z.max(b.z));
            for bx in bx0..=bx1 {
                for bz in bz0..=bz1 {
                    buckets.entry((bx, bz)).or_default().push(idx as u32);
                }
            }
        }

        Self { walls, buckets }
    }

    /// Distance from a point to the nearest wall (XZ) - half the free width
    /// there. Unbounded when no wall is near.
    fn clearance_at(&self, at: Vector3<f32>) -> f32 {
        let (bx, bz) = bucket_of(at.x, at.z);
        let mut nearest = f32::INFINITY;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(candidates) = self.buckets.get(&(bx + dx, bz + dz)) else {
                    continue;
                };
                for &idx in candidates {
                    let (wa, wb) = self.walls[idx as usize];
                    if (wa.y - at.y).abs() > WALL_FLOOR_SPAN
                        && (wb.y - at.y).abs() > WALL_FLOOR_SPAN
                    {
                        continue;
                    }
                    let closest = closest_point_on_segment_xz(wa, wb, at);
                    let (dx, dz) = (closest.x - at.x, closest.z - at.z);
                    nearest = nearest.min((dx * dx + dz * dz).sqrt());
                }
            }
        }
        nearest
    }

    /// Free half-width at each link's portal, parallel to `db.links`: the
    /// body crosses wherever the portal is roomiest (a portal at a cell
    /// corner is open on the corner side), so the best point along the
    /// shared edge wins.
    fn portal_clearances(&self, db: &PathDatabase) -> Vec<f32> {
        const SAMPLES: usize = 9;
        db.links
            .iter()
            .map(|link| {
                let (Some(a), Some(b)) = (
                    db.vertices.get(link.edge_vertex_a as usize),
                    db.vertices.get(link.edge_vertex_b as usize),
                ) else {
                    return f32::INFINITY;
                };
                (0..SAMPLES)
                    .map(|i| {
                        let t = i as f32 / (SAMPLES - 1) as f32;
                        self.clearance_at(a + (b - a) * t)
                    })
                    .fold(0.0f32, f32::max)
            })
            .collect()
    }

    /// Free half-width at each cell's center, parallel to `db.cells` - can a
    /// body stand there at all?
    fn cell_clearances(&self, db: &PathDatabase) -> Vec<f32> {
        db.cells
            .iter()
            .map(|cell| self.clearance_at(cell.center))
            .collect()
    }
}

/// Closest point to `target` on segment `a`-`b`, in the XZ plane
fn closest_point_on_segment_xz(
    a: Vector3<f32>,
    b: Vector3<f32>,
    target: Vector3<f32>,
) -> Vector3<f32> {
    let (dx, dz) = (b.x - a.x, b.z - a.z);
    let len_sq = dx * dx + dz * dz;
    if len_sq < 1e-8 {
        return a;
    }
    let t = (((target.x - a.x) * dx + (target.z - a.z) * dz) / len_sq).clamp(0.0, 1.0);
    a + (b - a) * t
}

/// Shrink an edge toward its center by EDGE_CLEARANCE on each end, so taut
/// crossing points can't land on the endpoints (wall corners). Edges shorter
/// than twice the clearance collapse to their midpoint - the doorway is
/// narrower than the clearance, so the middle is the best available.
fn inset_edge(a: Vector3<f32>, b: Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
    let ab = b - a;
    let len = ab.magnitude();
    if len <= 2.0 * EDGE_CLEARANCE {
        let mid = a + ab * 0.5;
        (mid, mid)
    } else {
        let dir = ab / len;
        (a + dir * EDGE_CLEARANCE, b - dir * EDGE_CLEARANCE)
    }
}

/// Closest point to `target` on the segment from `a` to `b`
fn closest_point_on_segment(
    a: Vector3<f32>,
    b: Vector3<f32>,
    target: Vector3<f32>,
) -> Vector3<f32> {
    let ab = b - a;
    let len_sq = ab.magnitude2();
    if len_sq < 1e-8 {
        return a;
    }
    let t = ((target - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    a + ab * t
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cgmath::vec3;
    use dark::mission::path_database::{CellDoor, PathCell, PathCellLink};

    /// Three unit-square cells in a row along X: 0 -> 1 -> 2.
    /// The 1 -> 2 link is gated by STRESSED; everything else is plain WALK.
    pub(crate) fn three_cell_db(last_cell_flags: PathCellFlags) -> PathDatabase {
        let vertices = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 0.0, 0.0),
            vec3(2.0, 0.0, 2.0),
            vec3(0.0, 0.0, 2.0),
            vec3(4.0, 0.0, 0.0),
            vec3(4.0, 0.0, 2.0),
            vec3(6.0, 0.0, 0.0),
            vec3(6.0, 0.0, 2.0),
        ];
        let cells = vec![
            PathCell {
                id: 0,
                center: vec3(1.0, 0.0, 1.0),
                vertex_indices: vec![0, 1, 2, 3],
                flags: PathCellFlags::empty(),
            },
            PathCell {
                id: 1,
                center: vec3(3.0, 0.0, 1.0),
                vertex_indices: vec![1, 4, 5, 2],
                flags: PathCellFlags::empty(),
            },
            PathCell {
                id: 2,
                center: vec3(5.0, 0.0, 1.0),
                vertex_indices: vec![4, 6, 7, 5],
                flags: last_cell_flags,
            },
        ];
        let links = vec![
            PathCellLink {
                from_cell: 0,
                to_cell: 1,
                edge_vertex_a: 1,
                edge_vertex_b: 2,
                ok_bits: MovementBits::WALK,
                cost: 5,
            },
            PathCellLink {
                from_cell: 1,
                to_cell: 2,
                edge_vertex_a: 4,
                edge_vertex_b: 5,
                ok_bits: MovementBits::WALK | MovementBits::STRESSED,
                cost: 5,
            },
        ];
        PathDatabase {
            cells,
            vertices,
            links,
            cell_doors: Vec::new(),
            cell_zones: Vec::new(),
            zone_pairs: Vec::new(),
        }
    }

    fn service(db: PathDatabase) -> PathfindingService {
        PathfindingService::new(Arc::new(db))
    }

    #[test]
    fn doors_near_cell_finds_the_gating_door_within_reach() {
        // Cells 0 -> 1 -> 2 in a row; cell 2 is a below-door cell gated by
        // door object 99.
        let mut db = three_cell_db(PathCellFlags::empty());
        db.cells[2].flags = PathCellFlags::BELOW_DOOR;
        db.cell_doors.push(CellDoor { cell: 2, door: 99 });
        let service = service(db);

        // From cell 0 the door is two hops away - within the lookahead
        assert_eq!(service.doors_near_cell(0), vec![(99, vec3(5.0, 0.0, 1.0))]);
        // Standing in the door cell itself also reports it
        assert_eq!(service.doors_near_cell(2), vec![(99, vec3(5.0, 0.0, 1.0))]);
    }

    #[test]
    fn doors_near_cell_empty_without_door_data() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        assert!(service.doors_near_cell(0).is_empty());
    }

    #[test]
    fn condition_gated_link_rejected_for_plain_walk() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        // Link index 1 is the STRESSED-gated 1 -> 2 link
        assert!(
            !service.can_use_link(1, MovementBits::WALK),
            "STRESSED-gated link must not be usable by a calm AI"
        );
        assert!(
            service.can_use_link(1, MovementBits::WALK | MovementBits::STRESSED),
            "stressed AI can use the gated link"
        );
    }

    #[test]
    fn zone_pair_grants_cross_zone_zero_bit_link() {
        // The 1 -> 2 link carries no okBits at all, but cells 1 and 2 are in
        // different zones and the zone-pair table marks the pair walkable -
        // the pattern shipped v2.9 data uses for area crossings.
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links[1].ok_bits = MovementBits::empty();
        // Raise cell 2 and its edge high enough that the flat-seam repair
        // alone would NOT grant this link - only the zone pair does.
        let lift = 4.0 / SCALE_FACTOR + 2.0;
        db.cells[2].center.y += lift;
        for v in [4usize, 5, 6, 7] {
            db.vertices[v].y += lift;
        }
        db.cell_zones = vec![7, 7, 9];
        let without_pair = service(db.clone());
        assert!(
            without_pair
                .find_path(
                    vec3(1.0, 0.0, 1.0),
                    vec3(5.0, lift, 1.0),
                    MovementBits::WALK
                )
                .is_none(),
            "zero-bit cross-zone link must be rejected without a zone-pair grant"
        );

        db.zone_pairs = vec![(7, 9, MovementBits::WALK)];
        let with_pair = service(db);
        assert!(
            with_pair
                .find_path(
                    vec3(1.0, 0.0, 1.0),
                    vec3(5.0, lift, 1.0),
                    MovementBits::WALK
                )
                .is_some(),
            "zone-pair table must grant the zero-bit crossing"
        );
    }

    #[test]
    fn flat_zero_bit_seam_is_walkable_but_cliff_is_not() {
        // Flat seam: zero okBits, same floor height -> repaired to WALK
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links[1].ok_bits = MovementBits::empty();
        let flat = service(db.clone());
        assert!(
            flat.find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
                .is_some(),
            "flat zero-bit seam must be walkable"
        );

        // Cliff: same zero okBits, destination floor far below -> stays dead
        let drop = 4.0 / SCALE_FACTOR + 2.0;
        db.cells[2].center.y -= drop;
        for v in [4usize, 5, 6, 7] {
            db.vertices[v].y -= drop;
        }
        let cliff = service(db);
        assert!(
            cliff
                .find_path(
                    vec3(1.0, 0.0, 1.0),
                    vec3(5.0, -drop, 1.0),
                    MovementBits::WALK
                )
                .is_none(),
            "zero-bit cliff link must stay unwalkable"
        );
    }

    #[test]
    fn bridge_connects_islands_only_with_nav_bridges() {
        // Remove the 1 -> 2 link entirely: cell 2 becomes a separate island
        // one edge-gap away (shared boundary at x = 4).
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links.truncate(1);
        let faithful = PathfindingService::new(Arc::new(db.clone()));
        assert!(
            faithful
                .find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
                .is_none(),
            "islands must stay separate without nav_bridges"
        );

        let bridged = PathfindingService::with_nav_options(Arc::new(db), true, None);
        let path = bridged
            .find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
            .expect("nav_bridges must synthesize a crossing");
        assert_eq!(path[0], vec3(1.0, 0.0, 1.0));
        assert_eq!(*path.last().unwrap(), vec3(5.0, 0.0, 1.0));
    }

    #[test]
    fn locked_door_gates_its_below_door_cell() {
        // Cell 2 is below door 99. While the door is locked, A* must refuse
        // to enter it; clearing the lock re-opens the route.
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links[1].ok_bits = MovementBits::WALK;
        db.cells[2].flags = PathCellFlags::BELOW_DOOR;
        db.cell_doors.push(CellDoor { cell: 2, door: 99 });
        let service = service(db);

        let start = vec3(1.0, 0.0, 1.0);
        let goal = vec3(5.0, 0.0, 1.0);
        assert!(
            service.find_path(start, goal, MovementBits::WALK).is_some(),
            "unlocked door cell must be pathable"
        );

        service.set_blocked_doors([99].into());
        assert!(
            service.find_path(start, goal, MovementBits::WALK).is_none(),
            "locked door cell must be unpathable"
        );

        service.set_blocked_doors(Default::default());
        assert!(
            service.find_path(start, goal, MovementBits::WALK).is_some(),
            "unlocking must restore the route"
        );
    }

    #[test]
    fn bridging_skips_links_with_out_of_range_cell_ids() {
        // Shipped data contains malformed tail links whose cell ids point
        // past the cell array (observed in hydro1) - bridge computation must
        // skip them instead of panicking
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links.push(PathCellLink {
            from_cell: 61516,
            to_cell: 2,
            edge_vertex_a: 0,
            edge_vertex_b: 1,
            ok_bits: MovementBits::WALK,
            cost: 1,
        });
        db.links.push(PathCellLink {
            from_cell: 0,
            to_cell: 61516,
            edge_vertex_a: 0,
            edge_vertex_b: 1,
            ok_bits: MovementBits::WALK,
            cost: 1,
        });
        let service = PathfindingService::with_nav_options(Arc::new(db), true, None);
        assert!(
            service
                .find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
                .is_some(),
            "service must build and route despite malformed links"
        );
    }

    #[test]
    fn doors_near_cell_traverses_bridge_links_without_panicking() {
        // Bridge link indices point past the raw link array; the door
        // lookahead BFS must resolve them through the combined index space
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links.truncate(1); // cell 2 becomes an island, bridged below
        db.cells[2].flags = PathCellFlags::BELOW_DOOR;
        db.cell_doors.push(CellDoor { cell: 2, door: 99 });
        let service = PathfindingService::with_nav_options(Arc::new(db), true, None);
        assert_eq!(service.doors_near_cell(0), vec![(99, vec3(5.0, 0.0, 1.0))]);
    }

    #[test]
    fn find_path_toward_routes_to_closest_reachable_cell() {
        // Goal sits beyond cell 2, which is unreachable (link removed). The
        // partial path should end at cell 1's center - the closest reachable
        // cell to the goal - instead of failing outright.
        let mut db = three_cell_db(PathCellFlags::empty());
        db.links.truncate(1);
        let service = service(db);
        assert!(
            service
                .find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
                .is_none()
        );
        let partial = service
            .find_path_toward(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
            .expect("partial route must exist");
        assert_eq!(
            *partial.last().unwrap(),
            vec3(3.0, 0.0, 1.0),
            "partial path ends at the closest reachable cell center"
        );
        // Already standing in the closest reachable cell: nothing to do
        assert!(
            service
                .find_path_toward(vec3(3.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
                .is_none()
        );
    }

    #[test]
    fn calm_walk_falls_back_to_stressed_route() {
        // The only route to cell 2 crosses a STRESSED-gated link. A calm WALK
        // query should still find it via the stressed second pass.
        let service = service(three_cell_db(PathCellFlags::empty()));
        let path = service.find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK);
        assert!(
            path.is_some(),
            "calm AI should route through stressed links when no calm route exists"
        );
    }

    #[test]
    fn small_creatures_get_no_stressed_fallback() {
        let mut db = three_cell_db(PathCellFlags::empty());
        for link in &mut db.links {
            link.ok_bits |= MovementBits::SMALL_CREATURE;
        }
        let service = service(db);
        let path = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::SMALL_CREATURE,
        );
        assert!(
            path.is_none(),
            "small creatures must not take the stressed fallback route"
        );
    }

    #[test]
    fn condition_gated_link_usable_when_condition_matches() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let path = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::STRESSED,
        );
        assert!(path.is_some(), "stressed AI should traverse the gated link");
    }

    #[test]
    fn link_into_blocked_cell_rejected() {
        let service = service(three_cell_db(PathCellFlags::UNPATHABLE));
        let path = service.find_path(vec3(1.0, 0.0, 1.0), vec3(3.0, 0.0, 1.0), MovementBits::WALK);
        assert!(path.is_some(), "path to the middle cell still works");
        // Goal resolves to the unpathable cell; A* must refuse to enter it.
        let blocked = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(5.0, 0.0, 1.0),
            MovementBits::WALK | MovementBits::STRESSED,
        );
        assert!(
            blocked.is_none(),
            "unpathable destination cell must be rejected"
        );
    }

    #[test]
    fn waypoints_cross_shared_edges_not_cell_centers() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let start = vec3(1.0, 0.0, 1.0);
        let goal = vec3(5.0, 0.0, 1.0);
        let path = service
            .find_path(start, goal, MovementBits::WALK | MovementBits::STRESSED)
            .unwrap();
        assert_eq!(path.len(), 4, "start + 2 edge crossings + goal");
        assert_eq!(path[0], start);
        assert_eq!(path[3], goal);
        // Crossing points sit on the shared edges (x = 2 and x = 4)
        assert!((path[1].x - 2.0).abs() < 1e-5);
        assert!((path[2].x - 4.0).abs() < 1e-5);
        // Straight corridor: the taut path stays on the straight line z = 1
        assert!((path[1].z - 1.0).abs() < 1e-5);
        assert!((path[2].z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn taut_waypoints_keep_clearance_from_edge_endpoints() {
        // A goal hugging the corridor wall (z near 0) pulls the taut points
        // toward the edge endpoints - wall corners. The crossing points must
        // keep EDGE_CLEARANCE distance from the endpoints so the AI's body
        // doesn't drag through (or wedge on) the corner. The test edges span
        // z in [0, 2] - shorter than twice the clearance - so they collapse
        // to their midpoint, the maximum available clearance.
        assert!(2.0 < 2.0 * EDGE_CLEARANCE, "test assumes short edges");
        let service = service(three_cell_db(PathCellFlags::empty()));
        let start = vec3(1.0, 0.0, 1.0);
        let goal = vec3(5.0, 0.0, 0.05);
        let path = service
            .find_path(start, goal, MovementBits::WALK | MovementBits::STRESSED)
            .unwrap();
        assert_eq!(path.len(), 4, "start + 2 edge crossings + goal");
        assert!(
            (path[1].z - 1.0).abs() < 1e-5,
            "crossing 1 not collapsed to the edge midpoint: z = {}",
            path[1].z
        );
        assert!(
            (path[2].z - 1.0).abs() < 1e-5,
            "crossing 2 not collapsed to the edge midpoint: z = {}",
            path[2].z
        );
    }

    #[test]
    fn stats_count_queries_retries_and_no_route() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        assert_eq!(service.stats().queries, 0);

        // Plain success: no retry, no no-route
        service
            .find_path(vec3(1.0, 0.0, 1.0), vec3(3.0, 0.0, 1.0), MovementBits::WALK)
            .unwrap();
        assert_eq!(
            service.stats(),
            PathfindingStats {
                queries: 1,
                stressed_retries: 0,
                no_route: 0
            }
        );

        // Only route is stressed-gated: counts a retry, still succeeds
        service
            .find_path(vec3(1.0, 0.0, 1.0), vec3(5.0, 0.0, 1.0), MovementBits::WALK)
            .unwrap();
        assert_eq!(
            service.stats(),
            PathfindingStats {
                queries: 2,
                stressed_retries: 1,
                no_route: 0
            }
        );

        // Goal outside every cell: both passes fail
        let miss = service.find_path(
            vec3(1.0, 0.0, 1.0),
            vec3(50.0, 0.0, 50.0),
            MovementBits::WALK,
        );
        assert!(miss.is_none());
        assert_eq!(
            service.stats(),
            PathfindingStats {
                queries: 3,
                stressed_retries: 2,
                no_route: 1
            }
        );
    }

    #[test]
    fn frame_budget_acquires_until_exhausted_and_resets() {
        let budget = PathfindingFrameBudget::new();
        for _ in 0..PATHFINDING_QUERIES_PER_FRAME {
            assert!(budget.try_acquire());
        }
        assert!(!budget.try_acquire(), "budget must exhaust");
        budget.reset();
        assert!(budget.try_acquire(), "reset must refill the budget");
    }

    #[test]
    fn reported_blocked_link_excludes_that_crossing_from_queries() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        let start = vec3(1.0, 0.0, 1.0);
        let goal = vec3(5.0, 0.0, 1.0);
        // Without a report the route crosses 0 -> 1 -> 2 normally
        assert!(service.find_path(start, goal, MovementBits::WALK).is_some());

        // A stall reported against the 0 -> 1 crossing severs the only
        // route: the full search fails and the partial fallback reports no
        // progress possible (the AI parks instead of grinding into the
        // obstacle - issue #481). The exclusion is SHARED - the blockage is
        // a physical fact, and it must protect fresh arrivals too.
        service.report_blocked_link(0, 1, 0.0);
        let avoid = service.blocked_links(1.0);
        assert!(avoid.contains(&(0, 1)));
        assert!(
            service
                .find_path_avoiding(start, goal, MovementBits::WALK, &avoid)
                .is_none()
        );
        assert!(
            service
                .find_path_toward_avoiding(start, goal, MovementBits::WALK, &avoid)
                .is_none()
        );

        // Only that DIRECTED crossing is excluded - a route already past it
        // (start in cell 1) still enters cell 2
        assert!(
            service
                .find_path_avoiding(vec3(3.0, 0.0, 1.0), goal, MovementBits::WALK, &avoid)
                .is_some()
        );
    }

    #[test]
    fn blocked_links_expire_after_their_ttl() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        service.report_blocked_link(0, 1, 100.0);
        assert!(
            service
                .blocked_links(100.0 + BLOCKED_LINK_TTL_SECONDS - 1.0)
                .contains(&(0, 1))
        );
        let expired = service.blocked_links(100.0 + BLOCKED_LINK_TTL_SECONDS + 1.0);
        assert!(expired.is_empty(), "entries must expire: {expired:?}");
        // The route is usable again once the entry expired
        assert!(
            service
                .find_path_avoiding(
                    vec3(1.0, 0.0, 1.0),
                    vec3(5.0, 0.0, 1.0),
                    MovementBits::WALK,
                    &expired
                )
                .is_some()
        );
    }

    #[test]
    fn prune_drops_steering_with_the_path_record() {
        let service = service(three_cell_db(PathCellFlags::empty()));
        service.record_ai_path(
            7,
            AiPathRecord {
                goal: vec3(5.0, 0.0, 1.0),
                waypoints: vec![vec3(1.0, 0.0, 1.0)],
                outcome: AiPathOutcome::Full,
            },
        );
        service.record_ai_steering(
            7,
            AiSteeringDebug {
                next_waypoint: 0,
                path_len: 1,
                target: None,
                stall_seconds: 0.0,
            },
        );
        service.report_blocked_link(0, 1, 0.0);

        // Entity 7 died (or despawned): its per-AI records go with it, but
        // reported blockages are world facts and stay until their TTL
        service.prune_ai_paths(|entity| entity != 7);
        assert!(service.ai_paths().is_empty());
        assert!(service.ai_steering(7).is_none());
        assert!(service.blocked_links(0.0).contains(&(0, 1)));
    }

    #[test]
    fn cell_lookup_prefers_matching_floor() {
        let mut db = three_cell_db(PathCellFlags::empty());
        // A second floor directly above cell 0
        let base = db.vertices.len() as u32;
        db.vertices.extend([
            vec3(0.0, 10.0, 0.0),
            vec3(2.0, 10.0, 0.0),
            vec3(2.0, 10.0, 2.0),
            vec3(0.0, 10.0, 2.0),
        ]);
        db.cells.push(PathCell {
            id: 3,
            center: vec3(1.0, 10.0, 1.0),
            vertex_indices: vec![base, base + 1, base + 2, base + 3],
            flags: PathCellFlags::empty(),
        });
        let service = service(db);
        assert_eq!(service.cell_from_position(vec3(1.0, 0.5, 1.0)), Some(0));
        assert_eq!(service.cell_from_position(vec3(1.0, 9.5, 1.0)), Some(3));
    }
}
