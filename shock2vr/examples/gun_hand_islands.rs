//! Scratch tool: report the material table and the baked-hand islands of the
//! static first-person gun models (`obj/*_h.bin`).
//!
//! VR draws the glove on a wielded gun and strips the baked hand/arm, and both
//! halves are decided by *material*: which slots the arm uses, and which arm
//! island is the trigger hand. This prints exactly that, so the classifier in
//! `dark::importers::is_first_person_arm_material` and the trigger-hand pick
//! are reviewable against the art instead of asserted.
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
            let span = |m: &ss2_bin_obj_loader::SystemShock2ObjectMesh| {
                let mut lo = [f32::INFINITY; 3];
                let mut hi = [f32::NEG_INFINITY; 3];
                for polygon in &m.polygons {
                    for index in &polygon.vertex_indices {
                        let p = m.vertices[*index as usize];
                        for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
                            lo[axis] = lo[axis].min(value);
                            hi[axis] = hi[axis].max(value);
                        }
                    }
                }
                (0..3)
                    .map(|axis| hi[axis] - lo[axis])
                    .fold(0.0_f32, f32::max)
            };
            let weapon = ss2_bin_obj_loader::retain_materials(mesh.clone(), |name| {
                !dark::importers::is_first_person_arm_material(name)
            });
            let view = span(&weapon);
            let world = name
                .strip_suffix("_h")
                .and_then(|stem| cache.get_raw_reader(&format!("{stem}_w.bin")))
                .map(|reader| {
                    let mut reader = reader.borrow_mut();
                    let header = dark::ss2_bin_header::read(&mut *reader);
                    span(&ss2_bin_obj_loader::read(&mut *reader, &header))
                });
            match world {
                Some(world) => println!(
                    "   weapon span {view:.3} units vs world model {world:.3} ({:.2}x)",
                    view / world
                ),
                None => println!("   weapon span {view:.3} units (no _w world model)"),
            }
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
            // Extent of the island's own geometry, to check the baked hand
            // against a real hand (`HAND_LENGTH_WORLD`, 0.2493 units ~ 19 cm).
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for polygon in &island.polygons {
                for index in &polygon.vertex_indices {
                    let p = island.vertices[*index as usize];
                    for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
                        lo[axis] = lo[axis].min(value);
                        hi[axis] = hi[axis].max(value);
                    }
                }
            }
            println!(
                "   island {index}: extent ({:.3},{:.3},{:.3})",
                hi[0] - lo[0],
                hi[1] - lo[1],
                hi[2] - lo[2]
            );
            match ss2_bin_obj_loader::hand_frame(
                island,
                dark::importers::is_first_person_arm_material,
            ) {
                Some(frame) => println!(
                    "   island {index}: polys {:4} [{materials}] wrist ({:7.3},{:7.3},{:7.3}) forward ({:6.3},{:6.3},{:6.3}) len {:.3}",
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
