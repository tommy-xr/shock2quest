//! Scratch tool: print the world-space span of the models the grip test cases
//! are held by, so a held item can be checked against a sensible real size.
//!
//! ```bash
//! DARK_ASSET_PATH=... cargo run -p shock2vr --example grip_measure
//! ```

use dark::importers::VR_CONTACT_MESH_IMPORTER;
use engine::assets::{asset_cache::AssetCache, asset_paths::AssetPath};

const ITEMS: &[&str] = &[
    "mug", "magci", "ammoss", "hamball", "atek_h", "atek_w", "sg_h", "sg_w", "fsn_h", "fsn_w",
    "al_h", "al_w", "wrench_w",
];

fn main() {
    let data_root = shock2vr::paths::data_root();
    // The glove ships as a bundle asset; off-device the repo's own folder is it.
    let mut mounts = vec![
        AssetPath::folder(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets").to_owned()),
        shock2vr::resource_family_paths("mesh"),
    ];
    mounts.extend(shock2vr::data_files::data_file_mounts(&data_root));
    mounts.push(AssetPath::folder("".to_owned()));
    let mut cache = AssetCache::new(
        data_root.to_string_lossy().into_owned(),
        AssetPath::combine(mounts),
    );

    let m = shock2vr::METERS_PER_WORLD_UNIT;
    for name in ITEMS {
        let file = format!("{name}.BIN");
        let Some(mesh) =
            cache.get_opt::<_, dark::importers::VrContactMesh, _>(&VR_CONTACT_MESH_IMPORTER, &file)
        else {
            println!("{name:10}: no mesh");
            continue;
        };
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for tri in &mesh.0 {
            for c in tri {
                for (i, v) in [c.x, c.y, c.z].iter().enumerate() {
                    min[i] = min[i].min(*v);
                    max[i] = max[i].max(*v);
                }
            }
        }
        if !min[0].is_finite() {
            println!("{name:10}: empty mesh");
            continue;
        }
        println!(
            "{name:10}: {} tris  span m ({:6.3}, {:6.3}, {:6.3})  centre ({:6.3}, {:6.3}, {:6.3})",
            mesh.0.len(),
            (max[0] - min[0]) * m,
            (max[1] - min[1]) * m,
            (max[2] - min[2]) * m,
            (min[0] + max[0]) * 0.5 * m,
            (min[1] + max[1]) * 0.5 * m,
            (min[2] + max[2]) * 0.5 * m,
        );
    }
}
