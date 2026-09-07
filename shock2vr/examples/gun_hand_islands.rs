//! Scratch tool: report the material table, the baked-hand islands and the
//! side-on silhouette of the static first-person gun models (`obj/*_h.bin`).
//!
//! VR draws the glove on a wielded gun and strips the baked hand/arm, and both
//! halves are decided by *material*: which slots the arm uses, and which arm
//! island is the trigger hand. This prints exactly that, so the classifier in
//! `dark::importers::is_first_person_arm_material` and the trigger-hand pick
//! are reviewable against the art instead of asserted.
//!
//! The silhouette is how `assets/vr_grips.json`'s gun grips are
//! placed: it draws the weapon-only geometry in its own authored frame (barrel
//! along -X, +Y up) on a labelled grid, so the pistol grip can be read off in
//! model units instead of guessed from a screenshot. `o` marks the model
//! origin, which the grip offset is measured from.
//!
//! ```bash
//! cargo run -p shock2vr --example gun_hand_islands
//! ```

use dark::ss2_bin_obj_loader;
use engine::assets::{asset_cache::AssetCache, asset_paths::AssetPath};

/// Every `_h` view model VR wields as a rigid gun (the melee `_h` set is
/// skinned and handled elsewhere).
const GUNS: &[&str] = &[
    "atek_h", "ar15_h", "sg_h", "lasehand", "empgun_h", "gren_h", "sfg_h", "fsn_h", "al_h",
    "viro_h", "amp_h",
];

/// Bounds and centroid of the geometry a mesh's polygons actually reference -
/// the one pass every readout below is derived from. `None` for a mesh whose
/// polygons reference nothing.
struct MeshBounds {
    lo: [f32; 3],
    hi: [f32; 3],
    centroid: [f32; 3],
}

fn mesh_bounds(mesh: &ss2_bin_obj_loader::SystemShock2ObjectMesh) -> Option<MeshBounds> {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let mut sum = [0.0f32; 3];
    let mut count = 0.0f32;
    for polygon in &mesh.polygons {
        for index in &polygon.vertex_indices {
            let vertex = mesh.vertices[*index as usize];
            for (axis, value) in [vertex.x, vertex.y, vertex.z].into_iter().enumerate() {
                lo[axis] = lo[axis].min(value);
                hi[axis] = hi[axis].max(value);
                sum[axis] += value;
            }
            count += 1.0;
        }
    }
    (count > 0.0).then(|| MeshBounds {
        lo,
        hi,
        centroid: [sum[0] / count, sum[1] / count, sum[2] / count],
    })
}

/// The longest axis of a mesh, in world units - the one number that says how
/// big a model is drawn.
fn longest_span(mesh: &ss2_bin_obj_loader::SystemShock2ObjectMesh) -> f32 {
    let Some(bounds) = mesh_bounds(mesh) else {
        return 0.0;
    };
    (0..3)
        .map(|axis| bounds.hi[axis] - bounds.lo[axis])
        .fold(0.0, f32::max)
}

/// The mesh's XY silhouette (barrel along -X, +Y up) on a coarse grid, with the
/// model origin marked - a readable map of where a gun's grip, trigger guard
/// and magazine actually are in the frame the wield's grip offset is expressed
/// in. Polygon edges are rasterized, not just their vertices, so a thin part
/// like a trigger guard still draws.
fn print_silhouette(mesh: &ss2_bin_obj_loader::SystemShock2ObjectMesh) {
    const COLS: usize = 78;
    const ROWS: usize = 24;

    let Some(MeshBounds { lo, hi, .. }) = mesh_bounds(mesh) else {
        return;
    };
    let mut edges = Vec::new();
    for polygon in &mesh.polygons {
        let corners = polygon
            .vertex_indices
            .iter()
            .map(|index| mesh.vertices[*index as usize])
            .collect::<Vec<_>>();
        for (index, corner) in corners.iter().enumerate() {
            edges.push((*corner, corners[(index + 1) % corners.len()]));
        }
    }
    // Square cells, so the picture is not stretched: one span drives both axes.
    let span = (hi[0] - lo[0]).max((hi[1] - lo[1]) * COLS as f32 / ROWS as f32);
    let cell = span / COLS as f32;
    let x0 = (lo[0] + hi[0]) / 2.0 - span / 2.0;
    let y_top = (lo[1] + hi[1]) / 2.0 + (ROWS as f32 / 2.0) * cell;

    let mut grid = vec![vec![b' '; COLS]; ROWS];
    let mut plot = |x: f32, y: f32, mark: u8| {
        let c = ((x - x0) / cell).floor() as isize;
        let r = ((y_top - y) / cell).floor() as isize;
        if (0..COLS as isize).contains(&c) && (0..ROWS as isize).contains(&r) {
            grid[r as usize][c as usize] = mark;
        }
    };
    for (a, b) in &edges {
        let steps = (((b.x - a.x).abs().max((b.y - a.y).abs())) / cell).ceil() as usize + 1;
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            plot(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, b'#');
        }
    }
    plot(0.0, 0.0, b'o');

    println!("   silhouette: col 0 = x {x0:+.3}, row 0 = y {y_top:+.3}, cell {cell:.4} units");
    for (r, row) in grid.iter().enumerate() {
        let y = y_top - (r as f32 + 0.5) * cell;
        println!("   {y:+6.3} |{}|", String::from_utf8_lossy(row));
    }
    let tens: String = (0..COLS)
        .map(|c| {
            if c % 10 == 0 {
                char::from(b'0' + (c / 10) as u8)
            } else {
                ' '
            }
        })
        .collect();
    println!("           {tens}   (x = {x0:+.3} + col * {cell:.4})");
}

/// A mesh's centroid and bounds - for an arm island, where the baked hand is.
fn print_bounds(label: &str, mesh: &ss2_bin_obj_loader::SystemShock2ObjectMesh) {
    let Some(MeshBounds { lo, hi, centroid }) = mesh_bounds(mesh) else {
        return;
    };
    println!(
        "   {label}: centroid ({:+.3},{:+.3},{:+.3}) bounds x[{:+.3},{:+.3}] y[{:+.3},{:+.3}] z[{:+.3},{:+.3}]",
        centroid[0], centroid[1], centroid[2], lo[0], hi[0], lo[1], hi[1], lo[2], hi[2],
    );
}

fn main() {
    let data_root = shock2vr::paths::data_root();
    let mut mounts = vec![shock2vr::resource_family_paths("obj")];
    mounts.extend(shock2vr::data_files::data_file_mounts(&data_root));
    mounts.push(AssetPath::folder("".to_owned()));
    let cache = AssetCache::new(
        data_root.to_string_lossy().into_owned(),
        AssetPath::combine(mounts),
    );

    for name in GUNS {
        let file = format!("{name}.bin");
        let Some(reader) = cache.get_raw_reader(&file) else {
            println!("== {name}: NOT FOUND");
            continue;
        };
        let mut reader = reader.borrow_mut();
        let header = dark::ss2_bin_header::read(&mut *reader);
        let mesh = ss2_bin_obj_loader::read(&mut *reader, &header);

        println!("== {name}");
        for material in &mesh.materials {
            let arm = dark::importers::is_first_person_arm_material(&material.name);
            let polys = mesh
                .polygons
                .iter()
                .filter(|polygon| polygon.slot_index == material.slot_num as u16)
                .count();
            println!(
                "   material slot {:2} {:<24} polys {:4} {}",
                material.slot_num,
                material.name,
                polys,
                if arm { "ARM" } else { "" }
            );
        }

        for vhot in &mesh.vhots {
            println!(
                "   vhot {:?} ({:7.3},{:7.3},{:7.3})",
                vhot.vhot_type, vhot.point.x, vhot.point.y, vhot.point.z
            );
        }

        // Weapon-only extent (arm materials excluded), against the true-scale
        // world model's - the view models are authored for a flat camera and
        // may be exaggerated.
        {
            let weapon = ss2_bin_obj_loader::retain_materials(mesh.clone(), |name| {
                !dark::importers::is_first_person_arm_material(name)
            });
            let view = longest_span(&weapon);
            let world = name
                .strip_suffix("_h")
                .and_then(|stem| cache.get_raw_reader(&format!("{stem}_w.bin")))
                .map(|reader| {
                    let mut reader = reader.borrow_mut();
                    let header = dark::ss2_bin_header::read(&mut *reader);
                    longest_span(&ss2_bin_obj_loader::read(&mut *reader, &header))
                });
            match world {
                Some(world) => println!(
                    "   weapon span {view:.3} units vs world model {world:.3} ({:.2}x)",
                    view / world
                ),
                None => println!("   weapon span {view:.3} units (no _w world model)"),
            }
        }

        {
            let weapon = ss2_bin_obj_loader::retain_materials(mesh.clone(), |name| {
                !dark::importers::is_first_person_arm_material(name)
            });
            print_bounds("weapon", &weapon);
            print_silhouette(&weapon);
        }

        let islands = ss2_bin_obj_loader::split_connected(
            &mesh,
            dark::importers::is_first_person_arm_material,
        );
        for (index, island) in islands.iter().enumerate() {
            let materials = island
                .materials
                .iter()
                .filter(|material| {
                    dark::importers::is_first_person_arm_material(&material.name)
                        && island
                            .polygons
                            .iter()
                            .any(|polygon| polygon.slot_index == material.slot_num as u16)
                })
                .map(|material| material.name.clone())
                .collect::<Vec<_>>()
                .join(",");
            // The island's own longest axis, to size the baked hand against a
            // real one (`HAND_LENGTH_WORLD`, 0.2493 units ~ 19 cm).
            println!("   island {index}: span {:.3} units", longest_span(island));
            print_bounds(&format!("island {index}"), island);
            match ss2_bin_obj_loader::hand_frame(
                island,
                dark::importers::is_first_person_arm_material,
            ) {
                Some(frame) => println!(
                    "   island {index}: polys {:4} [{materials}] far end ({:7.3},{:7.3},{:7.3}) forward ({:6.3},{:6.3},{:6.3}) len {:.3}",
                    island.polygons.len(),
                    frame.origin.x,
                    frame.origin.y,
                    frame.origin.z,
                    frame.forward.x,
                    frame.forward.y,
                    frame.forward.z,
                    frame.length,
                ),
                None => println!(
                    "   island {index}: polys {:4} [{materials}] no frame",
                    island.polygons.len()
                ),
            }
        }
    }
}
