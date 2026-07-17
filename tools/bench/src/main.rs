//! Benchmark and diagnostic CLI for engine subsystems.
//!
//! Benchmarks are namespaced by domain so new ones (mission loading, physics,
//! ...) can slot in beside `path`. The `path` domain loads the AIPATH chunk
//! straight from a mission file (no game session required) and exercises
//! `shock2vr::pathfinding::PathfindingService` the same way the game does,
//! reporting latency, success rate, and path-quality metrics so regressions
//! and improvements are measurable.
//!
//!   cargo bn path stats medsci1.mis   # database stats + flag/okBits audit
//!   cargo bn path bench medsci1.mis   # seeded A* benchmark
//!   cargo bn path bench --all --json  # all missions, machine-readable
//!   cargo bn path show medsci1.mis --from " -10,0,5" --to "20,0,30"
//!   cargo bn path cell medsci1.mis --at "20,0,30"

use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use cgmath::{InnerSpace, Vector3};
use clap::{Parser, Subcommand};
use dark::mission::path_database::{MovementBits, PathCellFlags, PathDatabase};
use dark::ss2_chunk_file_reader;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Serialize;
use shock2vr::pathfinding::PathfindingService;
use shock2vr::paths;

#[derive(Parser)]
#[command(name = "bench", about = "Benchmark and inspect engine subsystems")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// AIPATH pathfinding benchmarks and diagnostics
    #[command(subcommand)]
    Path(PathCommand),
}

#[derive(Subcommand)]
enum PathCommand {
    /// Print path database statistics and audit cell flags / link okBits
    Stats {
        /// Mission file name (e.g. medsci1.mis)
        mission: Option<String>,
        /// Run against every .mis file in Data/
        #[arg(long)]
        all: bool,
    },
    /// Run seeded random path queries and report latency + quality metrics
    Bench {
        /// Mission file name (e.g. medsci1.mis)
        mission: Option<String>,
        /// Run against every .mis file in Data/
        #[arg(long)]
        all: bool,
        /// Number of path queries per mission
        #[arg(long, default_value_t = 200)]
        queries: usize,
        /// RNG seed so runs are reproducible
        #[arg(long, default_value_t = 1138)]
        seed: u64,
        /// Emit JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
        /// Build the service with experimental island bridging (nav_bridges),
        /// so cross-area queries route and are measured
        #[arg(long)]
        bridges: bool,
    },
    /// Compute one path and dump its waypoints (investigation aid)
    Show {
        mission: String,
        /// Start position as "x,y,z" (world units, same as debug runtime)
        #[arg(long)]
        from: String,
        /// Goal position as "x,y,z"
        #[arg(long)]
        to: String,
    },
    /// List every cell containing a position, with flags and outgoing links
    Cell {
        mission: String,
        /// Position as "x,y,z"
        #[arg(long)]
        at: String,
    },
    /// Show the walk component containing a position and its frontier links
    /// (how the component connects - or fails to connect - to neighbors)
    Component {
        mission: String,
        /// Position as "x,y,z"
        #[arg(long)]
        at: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Path(command) => run_path_command(command),
    }
}

fn run_path_command(command: PathCommand) -> Result<()> {
    match command {
        PathCommand::Stats { mission, all } => {
            for mission in resolve_missions(mission, all)? {
                match load_path_database(&mission) {
                    Ok(db) => print_stats(&mission, db),
                    Err(e) => println!("{mission}: {e}"),
                }
            }
        }
        PathCommand::Bench {
            mission,
            all,
            queries,
            seed,
            json,
            bridges,
        } => {
            let mut reports = Vec::new();
            for mission in resolve_missions(mission, all)? {
                match load_path_database(&mission) {
                    Ok(db) => reports.extend(run_bench(&mission, db, queries, seed, bridges)),
                    Err(e) => {
                        if !json {
                            println!("{mission}: {e}");
                        }
                    }
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&reports)?);
            } else {
                print_bench_table(&reports);
            }
        }
        PathCommand::Show { mission, from, to } => {
            let db = load_path_database(&mission)?;
            dump_path(db, parse_vec3(&from)?, parse_vec3(&to)?);
        }
        PathCommand::Cell { mission, at } => {
            let db = load_path_database(&mission)?;
            dump_cells_at(db, parse_vec3(&at)?);
        }
        PathCommand::Component { mission, at } => {
            let db = load_path_database(&mission)?;
            dump_component_at(db, parse_vec3(&at)?);
        }
    }

    Ok(())
}

/// Show the walk component containing a position, plus every link that
/// leaves the component and why it doesn't extend it (okBits / dest flags).
fn dump_component_at(db: PathDatabase, at: Vector3<f32>) {
    let service = PathfindingService::new(Arc::new(db));
    let Some(cell_id) = service.cell_from_position(at) else {
        println!("no cell contains that position");
        return;
    };
    let db = &service.path_database;
    let components = walk_components(db);
    let Some(component) = components.iter().find(|c| c.contains(&cell_id)) else {
        println!("cell {cell_id} is not in any walk component (unpathable?)");
        return;
    };
    let members: std::collections::HashSet<u32> = component.iter().copied().collect();
    println!(
        "cell {cell_id} is in a walk component of {} cells",
        component.len()
    );

    // Collect frontier links: source inside, destination outside
    let mut frontier: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for link in &db.links {
        if !members.contains(&link.from_cell) || members.contains(&link.to_cell) {
            continue;
        }
        let dest_flags = db
            .cells
            .get(link.to_cell as usize)
            .map(|c| format!("{:?}", c.flags))
            .unwrap_or_else(|| "out-of-range".to_string());
        let key = format!(
            "okBits=[{}] dest_flags={}",
            describe_bits(link.ok_bits),
            dest_flags
        );
        *frontier.entry(key).or_default() += 1;
    }
    println!("frontier links (leaving the component): ");
    for (desc, count) in &frontier {
        println!("  {count:>5}  {desc}");
    }

    // How many frontier destinations are below-door cells the door table
    // maps to a door object? Those become passable when the door opens.
    let door_cells: std::collections::HashMap<u32, i32> =
        db.cell_doors.iter().map(|cd| (cd.cell, cd.door)).collect();
    let mut frontier_door_dests: std::collections::BTreeMap<i32, usize> =
        std::collections::BTreeMap::new();
    let mut frontier_blocking_non_door = 0usize;
    for link in &db.links {
        if !members.contains(&link.from_cell) || members.contains(&link.to_cell) {
            continue;
        }
        if let Some(&door) = door_cells.get(&link.to_cell) {
            *frontier_door_dests.entry(door).or_default() += 1;
        } else if db
            .cells
            .get(link.to_cell as usize)
            .map(|c| c.flags.contains(PathCellFlags::BLOCKING_OBB))
            .unwrap_or(false)
        {
            frontier_blocking_non_door += 1;
        }
    }
    println!(
        "frontier dests gated by a door: {} (across {} doors); blocking-obb non-door dests: {}",
        frontier_door_dests.values().sum::<usize>(),
        frontier_door_dests.len(),
        frontier_blocking_non_door
    );
    for (door, count) in frontier_door_dests.iter().take(8) {
        println!("    door obj {door}: {count} frontier links");
    }
}

fn resolve_missions(mission: Option<String>, all: bool) -> Result<Vec<String>> {
    if all {
        let mut missions: Vec<String> = std::fs::read_dir(paths::data_root())?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".mis"))
            .collect();
        missions.sort();
        Ok(missions)
    } else {
        mission
            .map(|m| vec![m])
            .ok_or_else(|| anyhow!("provide a mission file or --all"))
    }
}

fn load_path_database(mission: &str) -> Result<PathDatabase> {
    let path = paths::data_root().join(mission);
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut reader);
    PathDatabase::read(&toc, &mut reader)
        .ok_or_else(|| anyhow!("no usable AIPATH data (missing chunk or unsupported version)"))
}

fn parse_vec3(s: &str) -> Result<Vector3<f32>> {
    let parts: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<std::result::Result<_, _>>()
        .with_context(|| format!("expected \"x,y,z\", got {s:?}"))?;
    if parts.len() != 3 {
        return Err(anyhow!("expected 3 components, got {}", parts.len()));
    }
    Ok(Vector3::new(parts[0], parts[1], parts[2]))
}

// ---------------------------------------------------------------------------
// stats

fn print_stats(mission: &str, db: PathDatabase) {
    println!("=== {mission} ===");
    println!(
        "cells: {}  vertices: {}  links: {}",
        db.cells.len(),
        db.vertices.len(),
        db.links.len()
    );

    // Cell flag histogram
    let mut unpathable = 0;
    let mut below_door = 0;
    let mut blocking_obb = 0;
    let mut moving_terrain = 0;
    for cell in &db.cells {
        if cell.flags.contains(PathCellFlags::UNPATHABLE) {
            unpathable += 1;
        }
        if cell.flags.contains(PathCellFlags::BELOW_DOOR) {
            below_door += 1;
        }
        if cell.flags.contains(PathCellFlags::BLOCKING_OBB) {
            blocking_obb += 1;
        }
        if cell.flags.contains(PathCellFlags::MOVING_TERRAIN) {
            moving_terrain += 1;
        }
    }
    println!(
        "cell flags: unpathable={unpathable} below_door={below_door} blocking_obb={blocking_obb} moving_terrain={moving_terrain}"
    );

    // Link okBits histogram
    let mut by_bits: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for link in &db.links {
        *by_bits.entry(link.ok_bits.bits()).or_default() += 1;
    }
    println!("link okBits:");
    for (bits, count) in &by_bits {
        println!(
            "  0x{bits:02x} {:<40} {count}",
            describe_bits(MovementBits::from_bits_truncate(*bits))
        );
    }

    // Audit: links that can_use_link gates beyond plain movement-bit
    // matching (condition-only links and links into blocked cells).
    let mut walk_with_condition = 0;
    let mut into_blocked_cell = 0;
    for link in &db.links {
        let walkable_now = link.ok_bits.intersects(MovementBits::WALK);
        if !walkable_now {
            continue;
        }
        if link.ok_bits.intersects(MovementBits::CONDITION_MASK) {
            walk_with_condition += 1;
        }
        if let Some(dest) = db.cells.get(link.to_cell as usize) {
            if dest
                .flags
                .intersects(PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB)
            {
                into_blocked_cell += 1;
            }
        }
    }
    println!(
        "audit: walk links gated by a condition bit (rejected for calm AIs): {walk_with_condition}"
    );
    println!(
        "audit: walk links into unpathable/blocking-OBB cells (rejected): {into_blocked_cell}"
    );

    // Door audit: how are links into BELOW_DOOR cells tagged? If they carry
    // no okBits the mesh fragments at every doorway.
    let mut door_links_walkable = 0;
    let mut door_links_dead = 0;
    for link in &db.links {
        if let Some(dest) = db.cells.get(link.to_cell as usize) {
            if dest.flags.contains(PathCellFlags::BELOW_DOOR) {
                if link.ok_bits.intersects(MovementBits::WALK) {
                    door_links_walkable += 1;
                } else if link.ok_bits.is_empty() {
                    door_links_dead += 1;
                }
            }
        }
    }
    println!(
        "audit: links into below-door cells: walkable={door_links_walkable} zero-bits={door_links_dead}"
    );

    // Door table: below-door cells mapped to their gating door object
    let unique_doors: std::collections::HashSet<i32> =
        db.cell_doors.iter().map(|cd| cd.door).collect();
    let door_cells_also_blocking = db
        .cell_doors
        .iter()
        .filter(|cd| {
            db.cells
                .get(cd.cell as usize)
                .map(|c| c.flags.contains(PathCellFlags::BLOCKING_OBB))
                .unwrap_or(false)
        })
        .count();
    println!(
        "door table: {} cell-door entries, {} unique doors, {} of those cells also BLOCKING_OBB",
        db.cell_doors.len(),
        unique_doors.len(),
        door_cells_also_blocking
    );

    // Walk-graph connectivity for a plain WALK query. Links are treated as
    // undirected here, so this is weak connectivity (an upper bound on
    // mutual reachability). The original engine precomputes this as zone
    // tables; lots of tiny components are sliver cells (object tops, ledges)
    // reachable only via condition links.
    let components = walk_components(&db);
    let sizes: Vec<usize> = components.iter().map(|c| c.len()).collect();
    let isolated = sizes.iter().filter(|&&s| s == 1).count();
    println!(
        "walk connectivity: {} pathable cells in {} components; largest: {:?}; isolated cells: {}",
        components.iter().map(|c| c.len()).sum::<usize>(),
        components.len(),
        &sizes[..sizes.len().min(5)],
        isolated
    );
    println!();
}

/// Connected components of pathable cells over walk links (undirected),
/// sorted largest-first.
fn walk_components(db: &PathDatabase) -> Vec<Vec<u32>> {
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    let mut parent: Vec<u32> = (0..db.cells.len() as u32).collect();
    for link in &db.links {
        // Mirror PathfindingService::can_use_link for a plain WALK query:
        // walkable medium, no unsatisfied condition bits, unblocked dest.
        let walkable = link.ok_bits.intersects(MovementBits::WALK)
            && !link.ok_bits.intersects(MovementBits::CONDITION_MASK);
        let dest_ok = db
            .cells
            .get(link.to_cell as usize)
            .map(|c| {
                !c.flags
                    .intersects(PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB)
            })
            .unwrap_or(false);
        if walkable && dest_ok {
            let a = find(&mut parent, link.from_cell);
            let b = find(&mut parent, link.to_cell);
            parent[a as usize] = b;
        }
    }
    let mut by_root: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for &id in &pathable_cell_ids(db) {
        let root = find(&mut parent, id);
        by_root.entry(root).or_default().push(id);
    }
    let mut components: Vec<Vec<u32>> = by_root.into_values().collect();
    components.sort_unstable_by_key(|c| std::cmp::Reverse(c.len()));
    components
}

fn describe_bits(bits: MovementBits) -> String {
    if bits.is_empty() {
        return "(none)".to_string();
    }
    let names = [
        (MovementBits::WALK, "WALK"),
        (MovementBits::FLY, "FLY"),
        (MovementBits::SWIM, "SWIM"),
        (MovementBits::SMALL_CREATURE, "SMALL_CREATURE"),
        (MovementBits::STRESSED, "STRESSED"),
        (MovementBits::HIGH_STRIKE, "HIGH_STRIKE"),
        (MovementBits::APP1, "APP1"),
        (MovementBits::APP2, "APP2"),
    ];
    names
        .iter()
        .filter(|(bit, _)| bits.contains(*bit))
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join("|")
}

// ---------------------------------------------------------------------------
// bench

#[derive(Serialize)]
struct BenchReport {
    mission: String,
    cells: usize,
    links: usize,
    queries: usize,
    found: usize,
    no_path: usize,
    /// no_path broken down: lookup failed to resolve a cell vs A* exhausted
    miss_no_start_cell: usize,
    miss_no_goal_cell: usize,
    miss_no_route: usize,
    /// Queries where cell_from_position(cell center) returned a different
    /// cell than the one the center belongs to (floor ambiguity)
    lookup_mismatches: usize,
    lookup_us: LatencyStats,
    find_path_us: LatencyStats,
    /// find_path latency when no route exists (A* exhausts the component)
    unroutable_us: LatencyStats,
    /// find_path_toward (partial-route fallback) latency on those same
    /// cross-component queries - the full-component Dijkstra the game runs
    /// when a goal is unreachable
    toward_us: LatencyStats,
    /// Synthesized island-bridge links (0 without --bridges)
    bridge_links: usize,
    /// Service construction time (effective bits + bridging), milliseconds
    build_ms: u128,
    /// path length / straight-line distance, over successful queries
    mean_inflation: f32,
    p95_inflation: f32,
    /// total absolute heading change per path, degrees (zigzag indicator)
    mean_turn_deg: f32,
    mean_waypoints: f32,
}

#[derive(Serialize)]
struct LatencyStats {
    p50: u128,
    p95: u128,
    max: u128,
}

impl LatencyStats {
    fn from_samples(mut samples: Vec<u128>) -> LatencyStats {
        if samples.is_empty() {
            return LatencyStats {
                p50: 0,
                p95: 0,
                max: 0,
            };
        }
        samples.sort_unstable();
        let pick = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
        LatencyStats {
            p50: pick(0.50),
            p95: pick(0.95),
            max: *samples.last().unwrap(),
        }
    }
}

fn pathable_cell_ids(db: &PathDatabase) -> Vec<u32> {
    db.cells
        .iter()
        .filter(|cell| {
            !cell
                .flags
                .intersects(PathCellFlags::UNPATHABLE | PathCellFlags::BLOCKING_OBB)
                && cell.vertex_indices.len() >= 3
        })
        .map(|cell| cell.id)
        .collect()
}

fn run_bench(
    mission: &str,
    db: PathDatabase,
    queries: usize,
    seed: u64,
    bridges: bool,
) -> Option<BenchReport> {
    let cells = db.cells.len();
    let links = db.links.len();
    let components = walk_components(&db);
    let t_build = Instant::now();
    let service = PathfindingService::with_nav_options(Arc::new(db), bridges, None);
    let build_ms = t_build.elapsed().as_millis();
    let bridge_links = service.bridge_link_count();
    // Sample start/goal from the largest walk component so queries are
    // routable, like real AI-to-player queries. Cross-component (unroutable)
    // queries are timed separately below: they are the worst case because
    // A* must exhaust the whole component before giving up.
    let candidates = components.first().cloned().unwrap_or_default();
    if candidates.is_empty() {
        eprintln!("{mission}: no pathable walk cells; skipping bench");
        return None;
    }
    let mut rng = StdRng::seed_from_u64(seed);

    let mut found = 0;
    let mut no_path = 0;
    let mut miss_no_start_cell = 0;
    let mut miss_no_goal_cell = 0;
    let mut miss_no_route = 0;
    let mut lookup_mismatches = 0;
    let mut lookup_samples = Vec::with_capacity(queries);
    let mut path_samples = Vec::with_capacity(queries);
    let mut inflations = Vec::new();
    let mut turn_totals = Vec::new();
    let mut waypoint_counts = Vec::new();

    for _ in 0..queries {
        let start_id = candidates[rng.gen_range(0..candidates.len())];
        let goal_id = candidates[rng.gen_range(0..candidates.len())];
        let start = service.path_database.cells[start_id as usize].center;
        let goal = service.path_database.cells[goal_id as usize].center;

        let t = Instant::now();
        let resolved = service.cell_from_position(start);
        lookup_samples.push(t.elapsed().as_micros());
        if resolved != Some(start_id) {
            lookup_mismatches += 1;
        }

        let t = Instant::now();
        let path = service.find_path(start, goal, MovementBits::WALK);
        path_samples.push(t.elapsed().as_micros());

        match path {
            Some(waypoints) => {
                found += 1;
                waypoint_counts.push(waypoints.len() as f32);
                let straight = (goal - start).magnitude();
                let length: f32 = waypoints
                    .windows(2)
                    .map(|w| (w[1] - w[0]).magnitude())
                    .sum();
                if straight > 1.0 {
                    inflations.push(length / straight);
                }
                turn_totals.push(total_turn_degrees(&waypoints));
            }
            None => {
                no_path += 1;
                // Re-run the lookups to attribute the failure: did we fail to
                // resolve a cell at all, or did A* exhaust the open list?
                if service.cell_from_position(start).is_none() {
                    miss_no_start_cell += 1;
                } else if service.cell_from_position(goal).is_none() {
                    miss_no_goal_cell += 1;
                } else {
                    miss_no_route += 1;
                }
            }
        }
    }

    let mean = |v: &[f32]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f32>() / v.len() as f32
        }
    };
    let mut sorted_inflation = inflations.clone();
    sorted_inflation.sort_by(|a, b| a.total_cmp(b));
    let p95_inflation = sorted_inflation
        .get(((sorted_inflation.len().saturating_sub(1)) as f64 * 0.95) as usize)
        .copied()
        .unwrap_or(0.0);

    // Worst case: queries that cannot route (goal in another component).
    let mut unroutable_samples = Vec::new();
    let mut toward_samples = Vec::new();
    if components.len() > 1 {
        let other: Vec<u32> = components[1..].iter().flatten().copied().collect();
        for _ in 0..(queries / 10).max(1) {
            let start_id = candidates[rng.gen_range(0..candidates.len())];
            let goal_id = other[rng.gen_range(0..other.len())];
            let start = service.path_database.cells[start_id as usize].center;
            let goal = service.path_database.cells[goal_id as usize].center;
            let t = Instant::now();
            let _ = service.find_path(start, goal, MovementBits::WALK);
            unroutable_samples.push(t.elapsed().as_micros());

            let t = Instant::now();
            let _ = service.find_path_toward(start, goal, MovementBits::WALK);
            toward_samples.push(t.elapsed().as_micros());
        }
    }

    Some(BenchReport {
        mission: mission.to_string(),
        cells,
        links,
        queries,
        found,
        no_path,
        miss_no_start_cell,
        miss_no_goal_cell,
        miss_no_route,
        lookup_mismatches,
        lookup_us: LatencyStats::from_samples(lookup_samples),
        find_path_us: LatencyStats::from_samples(path_samples),
        unroutable_us: LatencyStats::from_samples(unroutable_samples),
        toward_us: LatencyStats::from_samples(toward_samples),
        bridge_links,
        build_ms,
        mean_inflation: mean(&inflations),
        p95_inflation,
        mean_turn_deg: mean(&turn_totals),
        mean_waypoints: mean(&waypoint_counts),
    })
}

/// Sum of absolute heading changes along the path in the XZ plane, degrees.
/// Straight corridors score near zero; zigzag/diamond paths score high.
fn total_turn_degrees(waypoints: &[Vector3<f32>]) -> f32 {
    let mut total = 0.0;
    for w in waypoints.windows(3) {
        let a = Vector3::new(w[1].x - w[0].x, 0.0, w[1].z - w[0].z);
        let b = Vector3::new(w[2].x - w[1].x, 0.0, w[2].z - w[1].z);
        if a.magnitude2() < 1e-6 || b.magnitude2() < 1e-6 {
            continue;
        }
        let cos = (a.dot(b) / (a.magnitude() * b.magnitude())).clamp(-1.0, 1.0);
        total += cos.acos().to_degrees();
    }
    total
}

fn print_bench_table(reports: &[BenchReport]) {
    println!(
        "{:<14} {:>6} {:>7} {:>6}/{:<5} {:>8} {:>11} {:>11} {:>11} {:>11} {:>8} {:>9} {:>7} {:>7} {:>7}",
        "mission",
        "cells",
        "links",
        "found",
        "miss",
        "badcell",
        "lookup p50",
        "path p95",
        "noroute p95",
        "toward p95",
        "inflate",
        "turn deg",
        "waypts",
        "bridges",
        "build"
    );
    for r in reports {
        println!(
            "{:<14} {:>6} {:>7} {:>6}/{:<5} {:>8} {:>10}u {:>10}u {:>10}u {:>10}u {:>8.2} {:>9.0} {:>7.1} {:>7} {:>6}m",
            r.mission,
            r.cells,
            r.links,
            r.found,
            r.no_path,
            r.lookup_mismatches,
            r.lookup_us.p50,
            r.find_path_us.p95,
            r.unroutable_us.p95,
            r.toward_us.p95,
            r.mean_inflation,
            r.mean_turn_deg,
            r.mean_waypoints,
            r.bridge_links,
            r.build_ms
        );
    }
}

// ---------------------------------------------------------------------------
// path / cell inspection

fn dump_path(db: PathDatabase, from: Vector3<f32>, to: Vector3<f32>) {
    let service = PathfindingService::new(Arc::new(db));
    let start_cell = service.cell_from_position(from);
    let goal_cell = service.cell_from_position(to);
    println!("start cell: {start_cell:?}  goal cell: {goal_cell:?}");

    match service.find_path(from, to, MovementBits::WALK) {
        Some(waypoints) => {
            let straight = (to - from).magnitude();
            let length: f32 = waypoints
                .windows(2)
                .map(|w| (w[1] - w[0]).magnitude())
                .sum();
            println!(
                "{} waypoints, path length {length:.2}, straight-line {straight:.2}, inflation {:.2}, total turn {:.0} deg",
                waypoints.len(),
                if straight > 0.01 {
                    length / straight
                } else {
                    0.0
                },
                total_turn_degrees(&waypoints)
            );
            for (i, wp) in waypoints.iter().enumerate() {
                let cell = service.cell_from_position(*wp);
                println!(
                    "  [{i:>3}] ({:>8.2}, {:>8.2}, {:>8.2})  cell {cell:?}",
                    wp.x, wp.y, wp.z
                );
            }
        }
        None => println!("no path found"),
    }
}

fn dump_cells_at(db: PathDatabase, at: Vector3<f32>) {
    let service = PathfindingService::new(Arc::new(db));
    let db = &service.path_database;

    let mut matches = Vec::new();
    for cell in &db.cells {
        // Reuse the service's notion of containment by probing each cell via
        // its own lookup: cell_from_position returns the first match only, so
        // collect all matches manually through the same 2D test the service
        // uses (point_in_cell is private; the XZ test below mirrors it).
        if cell.vertex_indices.len() < 3 {
            continue;
        }
        let verts: Vec<Vector3<f32>> = cell
            .vertex_indices
            .iter()
            .filter_map(|&i| db.vertices.get(i as usize))
            .copied()
            .collect();
        if verts.len() < 3 {
            continue;
        }
        let mut sign: Option<bool> = None;
        let mut inside = true;
        for i in 0..verts.len() {
            let v1 = verts[i];
            let v2 = verts[(i + 1) % verts.len()];
            let cross = (v2.x - v1.x) * (at.z - v1.z) - (v2.z - v1.z) * (at.x - v1.x);
            if cross.abs() < f32::EPSILON {
                continue;
            }
            let s = cross > 0.0;
            match sign {
                None => sign = Some(s),
                Some(prev) if prev != s => {
                    inside = false;
                    break;
                }
                _ => {}
            }
        }
        if inside {
            matches.push(cell);
        }
    }

    // Component membership helps explain "no path found" between two
    // positions that each look fine in isolation.
    let components = walk_components(db);
    let component_of = |cell_id: u32| -> Option<(usize, usize)> {
        components
            .iter()
            .enumerate()
            .find(|(_, c)| c.contains(&cell_id))
            .map(|(i, c)| (i, c.len()))
    };

    println!(
        "{} cell(s) contain ({:.2}, {:.2}, {:.2}) in the XZ plane:",
        matches.len(),
        at.x,
        at.y,
        at.z
    );
    for cell in matches {
        let component = component_of(cell.id)
            .map(|(idx, size)| format!("walk component #{idx} ({size} cells)"))
            .unwrap_or_else(|| "no walk component (unpathable?)".to_string());
        println!(
            "  cell {} center=({:.2}, {:.2}, {:.2}) flags={:?} {component}",
            cell.id, cell.center.x, cell.center.y, cell.center.z, cell.flags
        );
        for link in db.links.iter().filter(|l| l.from_cell == cell.id) {
            println!(
                "    -> cell {:<5} cost {:<3} [{}]",
                link.to_cell,
                link.cost,
                describe_bits(link.ok_bits)
            );
        }
    }
    println!(
        "service.cell_from_position would return: {:?}",
        service.cell_from_position(at)
    );
}
