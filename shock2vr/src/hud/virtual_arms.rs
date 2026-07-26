use cgmath::{Deg, Euler, Matrix4, Quaternion, Rotation, Vector3, vec3};
use dark::{
    importers::TEXTURE_IMPORTER,
    properties::{PropHitPoints, PropMaxHitPoints, PropPsiState},
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureOptions};
use shipyard::{Get, UniqueView, View, World};

use crate::{mission::PlayerInfo, vr_config::Handedness};

/// Offset from hand position to forearm HUD panel position
const FOREARM_OFFSET: Vector3<f32> = vec3(0.0, 0.0, 0.25); // 10cm toward elbow from hand

/// Size of the HUD panels (260x64 aspect ratio) - doubled in size
const HUD_PANEL_WIDTH: f32 = 0.26; // 26cm wide
const HUD_PANEL_HEIGHT: f32 = 0.064; // 6.4cm tall (260:64 = 4.0625:1 ratio)

/// BIOFULL.PCX texture dimensions
const BIOFULL_WIDTH: f32 = 260.0;
const BIOFULL_HEIGHT: f32 = 64.0;

const BAR_VERTICAL_OFFSET: f32 = -8.0;
const BAR_HORIZONTAL_OFFSET: f32 = 1.0;

/// Health bar overlay coordinates (pixel space on BIOFULL.PCX)
const HEALTH_BAR_START: (f32, f32) = (BAR_HORIZONTAL_OFFSET + 8.0, 40.0 + BAR_VERTICAL_OFFSET);
const HEALTH_BAR_END: (f32, f32) = (BAR_HORIZONTAL_OFFSET + 88.0, 54.0 + BAR_VERTICAL_OFFSET);

/// Psi bar overlay coordinates (pixel space on BIOFULL.PCX)
const PSI_BAR_START: (f32, f32) = (BAR_HORIZONTAL_OFFSET + 8.0, 17.0 + BAR_VERTICAL_OFFSET);
const PSI_BAR_END: (f32, f32) = (BAR_HORIZONTAL_OFFSET + 88.0, 31.0 + BAR_VERTICAL_OFFSET);

/// Z-offset for overlay layers to ensure proper rendering order
const OVERLAY_Z_OFFSET: f32 = 0.001;

/// Create HUD panels for both arms with health/psi overlays
pub fn create_arm_hud_panels(
    asset_cache: &mut AssetCache,
    world: &World,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
) -> Vec<SceneObject> {
    let mut scene_objects = Vec::new();

    // Create left arm HUD with health/psi overlays (BIOFULL base)
    let mut left_hud_layers = create_forearm_hud_with_overlays(
        asset_cache,
        world,
        left_hand_position,
        left_hand_rotation,
        Handedness::Left,
    );
    scene_objects.append(&mut left_hud_layers);

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

/// Convert pixel coordinates to UV coordinates (0.0 to 1.0)
fn pixel_to_uv(pixel_coords: (f32, f32)) -> (f32, f32) {
    (
        pixel_coords.0 / BIOFULL_WIDTH,
        pixel_coords.1 / BIOFULL_HEIGHT,
    )
}

/// Calculate overlay quad dimensions and position in world space
fn create_overlay_transform(
    base_position: Vector3<f32>,
    base_rotation: Quaternion<f32>,
    pixel_start: (f32, f32),
    pixel_end: (f32, f32),
    z_offset: f32,
) -> (Matrix4<f32>, f32, f32) {
    // Convert pixel coordinates to UV space
    let uv_start = pixel_to_uv(pixel_start);
    let uv_end = pixel_to_uv(pixel_end);

    // Calculate overlay dimensions as fraction of base HUD
    let overlay_width = (uv_end.0 - uv_start.0) * HUD_PANEL_WIDTH;
    let overlay_height = (uv_end.1 - uv_start.1) * HUD_PANEL_HEIGHT;

    // Calculate center offset relative to base HUD center
    let center_u = (uv_start.0 + uv_end.0) / 2.0 - 0.5; // -0.5 to center
    let center_v = (uv_start.1 + uv_end.1) / 2.0 - 0.5; // -0.5 to center

    // Convert UV offsets to world space offsets
    let offset_x = center_u * HUD_PANEL_WIDTH;
    let offset_y = -center_v * HUD_PANEL_HEIGHT; // Flip Y for correct orientation

    // Apply base rotation to offsets
    let local_offset = vec3(offset_x, offset_y, z_offset);
    let world_offset = base_rotation.rotate_vector(local_offset);

    let overlay_position = base_position + world_offset;

    let transform = Matrix4::from_translation(overlay_position)
        * Matrix4::from(base_rotation)
        * Matrix4::from_nonuniform_scale(overlay_width, overlay_height, 1.0);

    (transform, overlay_width, overlay_height)
}

/// Get player health percentage (0.0 to 1.0)
pub(crate) fn get_health_percentage(world: &World) -> f32 {
    // Get player entity from PlayerInfo
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let player_entity = player_info.entity_id;

    // Get current and max hit points
    let v_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();
    let v_max_hit_points = world.borrow::<View<PropMaxHitPoints>>().unwrap();

    if let (Ok(current_hp), Ok(max_hp)) = (
        v_hit_points.get(player_entity),
        v_max_hit_points.get(player_entity),
    ) {
        if max_hp.hit_points > 0 {
            (current_hp.hit_points as f32 / max_hp.hit_points as f32).clamp(0.0, 1.0)
        } else {
            1.0 // Default to full if no max HP set
        }
    } else {
        1.0 // Default to full if components not found
    }
}

/// Get player psi percentage (0.0 to 1.0)
pub(crate) fn get_psi_percentage(world: &World) -> f32 {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_psi = world.borrow::<View<PropPsiState>>().unwrap();

    if let Ok(psi) = v_psi.get(player_info.entity_id) {
        if psi.max_psi_points > 0 {
            return (psi.psi_points as f32 / psi.max_psi_points as f32).clamp(0.0, 1.0);
        }
    }
    0.0 // No psi pool - empty bar
}

/// The selected psi power to display in the HUD's weapon/ammo section:
/// `(discipline name, tier)`. `Some` only while the wielded weapon is the
/// psi amp (class tag `weapontype psiamp`).
pub(crate) fn get_wielded_psi_power(world: &World) -> Option<(String, i32)> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    let weapon = player_info.left_hand_entity_id?;

    // Is the wielded weapon the psi amp? Resolved via its template's class
    // tags, the same mechanism as `get_wielded_ammo_type`.
    let template_id = crate::scripts::script_util::entity_class_template_id(world, weapon)?;
    let class_tags = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateClassTags>>()
        .ok()?;
    if class_tags.0.get(&template_id)?.get("weapontype")? != "psiamp" {
        return None;
    }

    let powers = world
        .borrow::<UniqueView<crate::psi::GlobalPsiPowers>>()
        .ok()?;
    let selection = world
        .borrow::<UniqueView<crate::psi::PsiPowerSelection>>()
        .ok()?;
    let power = powers.0.get(selection.index)?;
    let name = power
        .display_name
        .clone()
        .unwrap_or_else(|| power.name.clone());
    Some((name, power.power.psi_cost))
}

/// The wielded psi amp's hold-to-overload meter state, or `None` when no
/// charge is in progress (or nothing is wielded).
pub(crate) fn get_wielded_psi_charge(
    world: &World,
) -> Option<crate::runtime_props::RuntimePropPsiCharge> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    let weapon = player_info.left_hand_entity_id?;
    let v_charge = world
        .borrow::<View<crate::runtime_props::RuntimePropPsiCharge>>()
        .ok()?;
    v_charge.get(weapon).ok().copied()
}

/// Current clip ammo of the wielded weapon, or `None` when unarmed or the held
/// item has no `PropGunState` (melee / unlimited debug weapons). In flatscreen
/// mode the wielded weapon is the player's left-hand slot (see
/// `FlatInteraction::held_entities`).
pub(crate) fn get_wielded_ammo(world: &World) -> Option<i32> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    let weapon = player_info.left_hand_entity_id?;
    let v_gun_state = world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()?;
    v_gun_state.get(weapon).ok().map(|g| g.ammo)
}

/// The template id of the wielded weapon's currently selected `Projectile` link
/// (its ammo type), or `None` when unarmed or the weapon has no projectile links
/// (melee). Honors `RuntimePropSelectedAmmo` (absent = the first link).
pub(crate) fn wielded_selected_projectile_template(world: &World) -> Option<i32> {
    let player_info = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    let weapon = player_info.left_hand_entity_id?;
    let projectiles = crate::scripts::script_util::ordered_projectile_links(world, weapon);
    if projectiles.is_empty() {
        return None;
    }
    let selected = world
        .borrow::<View<crate::runtime_props::RuntimePropSelectedAmmo>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|s| s.0))
        .unwrap_or(0);
    projectiles
        .get(selected % projectiles.len())
        .map(|(template_id, _)| *template_id)
}

/// The `ammotype` class-tag of the wielded weapon's selected ammo (e.g. "std",
/// "he", "ap"), or `None`.
pub(crate) fn get_wielded_ammo_type(world: &World) -> Option<String> {
    let template_id = wielded_selected_projectile_template(world)?;
    let class_tags = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateClassTags>>()
        .ok()?;
    class_tags.0.get(&template_id)?.get("ammotype").cloned()
}

/// The object-icon bitmap filename (e.g. "STD_I.pcx") of the wielded weapon's
/// selected ammo type, or `None`. Resolved from the selected projectile
/// template's `P$ObjIcon` (projectiles are templates, not instantiated entities,
/// so this reads the precomputed [`GlobalTemplateObjIcons`] map rather than a
/// `View`).
pub(crate) fn get_wielded_ammo_icon(world: &World) -> Option<String> {
    let template_id = wielded_selected_projectile_template(world)?;
    let icons = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateObjIcons>>()
        .ok()?;
    icons.0.get(&template_id).cloned()
}

/// Create layered forearm HUD with health and psi bar overlays
fn create_forearm_hud_with_overlays(
    asset_cache: &mut AssetCache,
    world: &World,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
) -> Vec<SceneObject> {
    let mut layers = Vec::new();

    // Only add overlays for left hand (BIOFULL display)
    if handedness != Handedness::Left {
        // For right hand, just create basic panel
        let panel = create_forearm_hud_panel(asset_cache, hand_position, hand_rotation, handedness);
        layers.push(panel);
        return layers;
    }

    // Calculate base HUD position and rotation
    let forearm_position = hand_position + hand_rotation.rotate_vector(FOREARM_OFFSET);
    let forearm_yaw_rotation = Quaternion::from(Euler::new(Deg(0.0), Deg(90.0), Deg(0.0)));
    let forearm_tilt_rotation = Quaternion::from(Euler::new(Deg(-90.0), Deg(0.0), Deg(180.0)));
    let final_rotation = hand_rotation * forearm_yaw_rotation * forearm_tilt_rotation;

    // Layer 1: Base BIOFULL panel
    let base_panel =
        create_forearm_hud_panel(asset_cache, hand_position, hand_rotation, handedness);
    layers.push(base_panel);

    // Layer 2: Health bar overlay
    let health_percentage = get_health_percentage(world);
    if let Some(health_overlay) = create_bar_overlay(
        asset_cache,
        "HPBAR.PCX",
        forearm_position,
        final_rotation,
        HEALTH_BAR_START,
        HEALTH_BAR_END,
        health_percentage,
        OVERLAY_Z_OFFSET,
    ) {
        layers.push(health_overlay);
    }

    // Layer 3: Psi bar overlay
    let psi_percentage = get_psi_percentage(world);
    if let Some(psi_overlay) = create_bar_overlay(
        asset_cache,
        "PSIBAR.PCX",
        forearm_position,
        final_rotation,
        PSI_BAR_START,
        PSI_BAR_END,
        psi_percentage,
        OVERLAY_Z_OFFSET * 2.0, // Stack above health bar
    ) {
        layers.push(psi_overlay);
    }

    layers
}

/// Create a clipped bar overlay at specific pixel coordinates
fn create_bar_overlay(
    asset_cache: &mut AssetCache,
    texture_name: &str,
    base_position: Vector3<f32>,
    base_rotation: Quaternion<f32>,
    pixel_start: (f32, f32),
    pixel_end: (f32, f32),
    clip_percentage: f32,
    z_offset: f32,
) -> Option<SceneObject> {
    // Load bar texture
    let texture_options = TextureOptions {
        wrap: false,
        ..Default::default()
    };
    let texture = asset_cache.get_ext(&TEXTURE_IMPORTER, texture_name, &texture_options);

    // Create clipped screen material
    let material = engine::scene::clipped_screen_material::create(
        texture.clone() as std::rc::Rc<dyn engine::texture::TextureTrait>,
        clip_percentage,
    );

    // Create geometry
    let geometry = Box::new(engine::scene::quad::create());

    // Calculate overlay transform
    let (transform, _width, _height) = create_overlay_transform(
        base_position,
        base_rotation,
        pixel_start,
        pixel_end,
        z_offset,
    );

    // Create scene object
    let mut scene_object = SceneObject::new(material, geometry);
    scene_object.set_transform(transform);

    Some(scene_object)
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
    let texture_options = TextureOptions {
        wrap: false,
        ..Default::default()
    };
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
