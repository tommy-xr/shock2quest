use cgmath::{Deg, Euler, Matrix4, Quaternion, Rotation, Vector3, vec3};
use dark::{
    importers::TEXTURE_IMPORTER,
    properties::{PropHitPoints, PropMaxHitPoints, PropPsiState},
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureOptions};
use shipyard::{Get, UniqueView, View, World};

use crate::{
    hud::{ammo_panel, readouts},
    mission::PlayerInfo,
    ui::UiCanvas,
    vr_config::Handedness,
};

/// Offset from hand position to forearm HUD panel *centre*. The panel's width
/// axis runs along the arm (hand-local +Z), so it spans
/// `FOREARM_OFFSET.z +/- HUD_PANEL_WIDTH / 2` and its near edge - not this
/// centre - is what the forearm geometry has to stop short of.
pub(crate) const FOREARM_OFFSET: Vector3<f32> = vec3(0.0, 0.0, 0.25); // world units: 0.25 * 0.762 = 19 cm toward the elbow

/// Size of the HUD panels (260x64 aspect ratio) - doubled in size. In world
/// units, like every other length here; `crate::METERS_PER_WORLD_UNIT` converts.
pub(crate) const HUD_PANEL_WIDTH: f32 = 0.26; // world units: 0.26 * 0.762 = 20 cm along the arm
const HUD_PANEL_HEIGHT: f32 = 0.064; // 6.4cm tall (260:64 = 4.0625:1 ratio)

/// Z-offset for overlay layers to ensure proper rendering order
const OVERLAY_Z_OFFSET: f32 = 0.001;

/// Create the forearm HUD panels: BIOFULL (health/psi) on the left arm,
/// AMMOFULL (the live ammo readout) on the right.
///
/// `use_mode` silences both: the cyber interface carries the expanded BIOFULL
/// and AMMOFULL readouts on its canvas while it is up (`hud::readouts`), so
/// leaving the arms lit would show a VR player the same numbers twice, at two
/// scales and orientations (issue #1268). Flat drops its compact overlay in
/// use mode for the same reason (`hud::flat_hud`).
pub fn create_arm_hud_panels(
    asset_cache: &mut AssetCache,
    world: &World,
    use_mode: bool,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
) -> Vec<SceneObject> {
    if use_mode {
        return Vec::new();
    }

    // The bio monitor on the left arm, the ammo gauge on the right - each the
    // shared layout's own canvas, hung by the one compositor below.
    let mut scene_objects = forearm_readout_panel(
        asset_cache,
        left_hand_position,
        left_hand_rotation,
        Handedness::Left,
        readouts::build_bio_readout_canvas(&readouts::BioReadout::from_world(world)),
    );
    scene_objects.append(&mut forearm_readout_panel(
        asset_cache,
        right_hand_position,
        right_hand_rotation,
        Handedness::Right,
        ammo_panel::build_readout_canvas(&ammo_panel::AmmoReadout::from_world(world, false)),
    ));

    // Part of the player's hand visuals: labelled here rather than at the call
    // sites so the `debug_hud` scene, which emits these without an interaction
    // controller, is covered by the pause menu's suppression too (issue #1018).
    crate::util::tag_render_source(&mut scene_objects, crate::util::render_source::PLAYER_HANDS);

    scene_objects
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
    // The psi amp is resolved via its template's class tags, the same mechanism
    // as `get_wielded_ammo_type`.
    crate::wielded_weapon::wielded_psi_amp(world)?;

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
    // The tier, not the psi-point cost: the badge art is `AmPsi<tier>1.PCX`.
    Some((name, power.tier()))
}

/// The wielded psi amp's hold-to-overload meter state, or `None` when no
/// charge is in progress (or nothing is wielded).
pub(crate) fn get_wielded_psi_charge(
    world: &World,
) -> Option<crate::runtime_props::RuntimePropPsiCharge> {
    let weapon = crate::wielded_weapon::wielded_weapon(world)?;
    let v_charge = world
        .borrow::<View<crate::runtime_props::RuntimePropPsiCharge>>()
        .ok()?;
    v_charge.get(weapon).ok().copied()
}

/// Current clip ammo of the wielded weapon, or `None` when unarmed or the held
/// item has no `PropGunState` (melee / unlimited debug weapons). Which held
/// entity counts as "the wielded weapon" - in either presentation, in either
/// hand - is decided by [`crate::wielded_weapon`].
pub(crate) fn get_wielded_ammo(world: &World) -> Option<i32> {
    let weapon = crate::wielded_weapon::wielded_weapon(world)?;
    let v_gun_state = world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()?;
    v_gun_state.get(weapon).ok().map(|g| g.ammo)
}

/// The template id of the wielded weapon's currently selected `Projectile` link
/// (its ammo type), or `None` when unarmed or the weapon has no projectile links
/// (melee). Honors `RuntimePropSelectedAmmo` (absent = the first link).
pub(crate) fn wielded_selected_projectile_template(world: &World) -> Option<i32> {
    let weapon = crate::wielded_weapon::wielded_weapon(world)?;
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

/// The wielded gun's fire setting: its index (0 or 1) and the short header for
/// that setting ("NORM" / "BURST"), or `None` when nothing gun-like is wielded.
/// The header is absent for a gun whose data names no header for the setting.
pub(crate) fn get_wielded_gun_setting(world: &World) -> Option<(i32, Option<String>)> {
    let weapon = crate::wielded_weapon::wielded_weapon(world)?;
    let setting = world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.setting))?;
    let header = crate::scripts::script_util::gun_setting_header(world, weapon, setting);
    Some((setting, header))
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

/// One forearm panel: the backdrop art for `handedness` with `readout` - a
/// panel-sized canvas drawn at panel origin (0,0) - composited one overlay step
/// in front of it.
///
/// The backdrop stays a plain lit quad rather than a canvas element (the canvas
/// presenter draws its elements fully emissive, which suits a readout but would
/// make the panel glow), and `readout` is whatever the SHARED presentation-
/// agnostic layout emitted, so an arm cannot drift from the interface canvas or
/// the flat HUD (AGENTS.md section 3). Both arms composite identically here, so
/// a change to how one is hung cannot miss the other.
///
/// No controls on either arm: the VR pointer only hits the cyber-interface
/// panel, never these quads, so drawing a SETTING/RELOAD/cycle button nobody can
/// press would be a lie. Only the interactive layer differs, and it differs by
/// whether a pointer can reach it, not by presentation.
fn forearm_readout_panel(
    asset_cache: &mut AssetCache,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
    readout: UiCanvas,
) -> Vec<SceneObject> {
    let mut objects = vec![create_forearm_hud_panel(
        asset_cache,
        hand_position,
        hand_rotation,
        handedness,
    )];

    objects.append(&mut readout.render_world_space(
        asset_cache,
        forearm_panel_transform(hand_position, hand_rotation, handedness)
            * Matrix4::from_translation(vec3(0.0, 0.0, OVERLAY_Z_OFFSET)),
        None,
        None,
        OVERLAY_Z_OFFSET,
    ));

    objects
}

/// Create a single forearm HUD panel
fn create_forearm_hud_panel(
    asset_cache: &mut AssetCache,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
) -> SceneObject {
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

    // Create scene object, placed by the shared forearm pose
    let mut scene_object = SceneObject::new(material, geometry);
    scene_object.set_transform(forearm_panel_transform(
        hand_position,
        hand_rotation,
        handedness,
    ));

    scene_object
}

/// Where a forearm panel hangs: offset from the hand toward the elbow, yawed
/// toward the body and tilted flat against the arm like a wrist computer.
/// The single source of forearm placement - the panel quad, its overlays and
/// the ammo readout canvas all derive from this, so they cannot drift apart.
fn forearm_pose(
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
) -> (Vector3<f32>, Quaternion<f32>) {
    let forearm_position = hand_position + hand_rotation.rotate_vector(FOREARM_OFFSET);
    let forearm_yaw_rotation = match handedness {
        Handedness::Left => Quaternion::from(Euler::new(Deg(0.0), Deg(90.0), Deg(0.0))),
        Handedness::Right => Quaternion::from(Euler::new(Deg(0.0), Deg(-90.0), Deg(0.0))),
    };
    let forearm_tilt_rotation = Quaternion::from(Euler::new(Deg(-90.0), Deg(0.0), Deg(180.0)));
    (
        forearm_position,
        hand_rotation * forearm_yaw_rotation * forearm_tilt_rotation,
    )
}

/// [`forearm_pose`] as a root transform for a panel-sized canvas, so canvas
/// elements land exactly on the panel quad.
fn forearm_panel_transform(
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    handedness: Handedness,
) -> Matrix4<f32> {
    let (position, rotation) = forearm_pose(hand_position, hand_rotation, handedness);
    Matrix4::from_translation(position)
        * Matrix4::from(rotation)
        * Matrix4::from_nonuniform_scale(HUD_PANEL_WIDTH, HUD_PANEL_HEIGHT, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::assets::asset_paths::AssetPath;

    /// With the cyber interface up, the forearms emit nothing at all - the
    /// interface canvas is the single copy of both readouts (issue #1268).
    ///
    /// The gate short-circuits before the world or the asset cache is touched,
    /// which is what makes an empty `World` (no `PlayerInfo`) and an asset path
    /// that resolves nothing a sufficient fixture: an ungated build reaches
    /// both and fails.
    #[test]
    fn the_forearms_go_quiet_while_use_mode_is_up() {
        // No mounts: every lookup misses, so touching the cache is the failure
        // this test is looking for.
        let mut assets = AssetCache::new(String::new(), AssetPath::combine(vec![]));
        let objects = create_arm_hud_panels(
            &mut assets,
            &World::new(),
            true,
            vec3(0.0, 0.0, 0.0),
            Quaternion::from(Euler::new(Deg(0.0), Deg(0.0), Deg(0.0))),
            vec3(0.0, 0.0, 0.0),
            Quaternion::from(Euler::new(Deg(0.0), Deg(0.0), Deg(0.0))),
        );
        assert!(objects.is_empty());
    }
}
