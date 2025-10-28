use cgmath::{vec3, Deg, Euler, Matrix4, Quaternion, Rotation, Vector3};
use dark::importers::TEXTURE_IMPORTER;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureOptions};

use crate::vr_config::Handedness;

/// Offset from hand position to forearm HUD panel position
const FOREARM_OFFSET: Vector3<f32> = vec3(0.0, 0.0, 0.25); // 10cm toward elbow from hand

/// Size of the HUD panels (260x64 aspect ratio) - doubled in size
const HUD_PANEL_WIDTH: f32 = 0.26; // 26cm wide
const HUD_PANEL_HEIGHT: f32 = 0.064; // 6.4cm tall (260:64 = 4.0625:1 ratio)

/// Create HUD panels for both arms
pub fn create_arm_hud_panels(
    asset_cache: &mut AssetCache,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
) -> Vec<SceneObject> {
    let mut scene_objects = Vec::new();

    // Create left arm HUD (BIOFULL - for health/bio)
    let left_hud = create_forearm_hud_panel(
        asset_cache,
        left_hand_position,
        left_hand_rotation,
        Handedness::Left,
    );
    scene_objects.push(left_hud);

    // Create right arm HUD (AMMOFULL - for ammo)
    let right_hud = create_forearm_hud_panel(
        asset_cache,
        right_hand_position,
        right_hand_rotation,
        Handedness::Right,
    );
    scene_objects.push(right_hud);

    scene_objects
}

/// Create a single forearm HUD panel
fn create_forearm_hud_panel(
    asset_cache: &mut AssetCache,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
) -> SceneObject {
    // Calculate forearm position - offset from hand toward elbow
    let forearm_position = hand_position + hand_rotation.rotate_vector(FOREARM_OFFSET);

    // Load appropriate texture based on handedness
    let texture_options = TextureOptions { wrap: false };
    let texture = match handedness {
        Handedness::Left => asset_cache.get_ext(&TEXTURE_IMPORTER, "BIOFULL.PCX", &texture_options),
        Handedness::Right => {
            asset_cache.get_ext(&TEXTURE_IMPORTER, "AMMOFULL.PCX", &texture_options)
        }
    };

    // Create BasicMaterial with the loaded texture (casting to the expected trait object)
    let material = engine::scene::basic_material::create(
        texture.clone() as std::rc::Rc<dyn engine::texture::TextureTrait>,
        0.0, // No emissivity
        0.0, // No transparency
    );

    // Create quad geometry
    let geometry = Box::new(engine::scene::quad::create());

    // Calculate wearable computer orientation
    // For a forearm-mounted display, we need additional rotations:
    // 1. Yaw rotation to align with forearm direction
    // 2. Z rotation to make it lie flat on the forearm like a wrist computer
    let forearm_yaw_rotation = match handedness {
        Handedness::Left => Quaternion::from(Euler::new(Deg(0.0), Deg(90.0), Deg(0.0))), // Rotate left panel toward body
        Handedness::Right => Quaternion::from(Euler::new(Deg(0.0), Deg(-90.0), Deg(0.0))), // Rotate right panel toward body
    };

    // Z rotation to tilt the panel flat against the forearm (like looking down at a wrist watch)
    let forearm_tilt_rotation = Quaternion::from(Euler::new(Deg(-90.0), Deg(0.0), Deg(180.0)));

    // Combine all rotations: hand rotation + yaw + tilt
    let final_rotation = hand_rotation * forearm_yaw_rotation * forearm_tilt_rotation;

    // Calculate transform matrix with proper aspect ratio and wearable orientation
    let transform = Matrix4::from_translation(forearm_position)
        * Matrix4::from(final_rotation)
        * Matrix4::from_nonuniform_scale(HUD_PANEL_WIDTH, HUD_PANEL_HEIGHT, 1.0);

    // Create scene object
    let mut scene_object = SceneObject::new(material, geometry);
    scene_object.set_transform(transform);

    scene_object
}
