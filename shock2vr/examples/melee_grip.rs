//! Scratch tool: print the posed joint positions of the melee first-person
//! (`_h`) skinned meshes at the player-melee idle's final frame - the pose VR
//! wields them in. The hand joint (`MELEE_GRIP_JOINT`) is what
//! `vr_config::melee_wield_pose_correction` cancels at runtime so the model's
//! baked fist lands on the tracked hand.
//!
//! It also prints each model's `melee_contact_offset` - where the rendered
//! weapon head lands in hand-local space, which is what the model's
//! `HAND_MODEL_POSITIONING` entry must be so the contact collider sits on the
//! weapon the player sees. Re-run this and paste the numbers into
//! `MELEE_CONTACT_*` if the rigs ever change.
//!
//! ```bash
//! DARK_ASSET_PATH=... cargo run -p shock2vr --example melee_grip
//! ```

use dark::importers::{ANIMATION_CLIP_IMPORTER, SKELETON_IMPORTER};
use dark::motion::AnimationPlayer;
use engine::assets::{asset_cache::AssetCache, asset_paths::AssetPath};

fn main() {
    let data_root = shock2vr::paths::data_root();
    let mut mounts = vec![
        shock2vr::resource_family_paths("mesh"),
        shock2vr::resource_family_paths("motions"),
    ];
    mounts.extend(shock2vr::data_files::data_file_mounts(&data_root));
    mounts.push(AssetPath::folder("".to_owned()));
    let mut cache = AssetCache::new(
        data_root.to_string_lossy().into_owned(),
        AssetPath::combine(mounts),
    );

    let clip = cache.get(&ANIMATION_CLIP_IMPORTER, "ph212203_.mc");
    let player = AnimationPlayer::with_root_motion_cancelled(
        &AnimationPlayer::from_completed_animation(clip),
    );

    for name in ["wrench_h", "rapier_h", "shard_h", "psword_h"] {
        let skeleton = cache.get(&SKELETON_IMPORTER, &format!("{name}.cal"));
        let joints = player.get_transforms(&skeleton);
        println!("== {name}");
        for (i, m) in joints.iter().enumerate() {
            let p = m.w;
            if p.x.abs() + p.y.abs() + p.z.abs() > 1e-6 {
                println!("  joint {i:2}: ({:8.4}, {:8.4}, {:8.4})", p.x, p.y, p.z);
            }
        }
        let contact = shock2vr::melee_contact_offset(shock2vr::MeleePosedArm::from_joints(&joints));
        println!(
            "  contact offset (hand-local): vec3({:.3}, {:.3}, {:.3})",
            contact.x, contact.y, contact.z
        );
    }
}
