//! The readouts the VR player wears on the wrists of the gloves.
//!
//! The left glove carries a **watch face** - health, the one stat worth a
//! glance mid-fight - and the right carries the ammo gauge. Each is a canvas
//! the shared layout built (`hud::readouts`, `hud::ammo_panel`), cropped to the
//! row or the well it shows; nothing here decides where a bar or a number goes,
//! only where the plate hangs on the hand (AGENTS.md section 3).

use cgmath::{Deg, Matrix4, Quaternion, Rotation, Vector2, Vector3, vec3};
use dark::properties::{PropHitPoints, PropMaxHitPoints, PropPsiState};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Get, UniqueView, View, World};

use crate::{
    hud::{ammo_panel, readouts},
    mission::PlayerInfo,
    ui::UiCanvas,
};

/// Where a wrist canvas hangs, in the hand's own frame. The hand points down
/// its -Z (the aim/raycast direction the glove's fingers follow), so +Z is
/// toward the elbow and this sits just past the glove's cuff, clear of the
/// emissive cuff ring; +Y is out of the back of the hand, so the plate rides on
/// top of the wrist and faces the player when the wrist is turned up.
const WRIST_OFFSET: Vector3<f32> = vec3(0.0, 0.02, 0.055);

/// How wide every wrist canvas is, in world units (`crate::METERS_PER_WORLD_UNIT`
/// converts: ~5.9 cm, a wrist's width). Height follows from each canvas's own
/// aspect, so the two wrists read as a matched pair whatever art each wears and
/// neither is stretched.
const WRIST_CANVAS_WIDTH: f32 = 0.078;

/// Z-offset for overlay layers to ensure proper rendering order
const OVERLAY_Z_OFFSET: f32 = 0.001;

/// Create the wrist HUD canvases: the health watch on the left glove, the ammo
/// gauge on the right.
///
/// `use_mode` silences both: the cyber interface carries the expanded BIOFULL
/// and AMMOFULL readouts on its canvas while it is up (`hud::readouts`), so
/// leaving the wrists lit would show a VR player the same numbers twice, at two
/// scales and orientations (issue #1268). Flat drops its compact overlay in
/// use mode for the same reason (`hud::flat_hud`).
pub fn create_wrist_hud_panels(
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

    // The watch on the left wrist, the ammo gauge on the right - each the
    // shared layout's own canvas, hung by the one compositor below.
    let mut scene_objects = wrist_canvas(
        asset_cache,
        left_hand_position,
        left_hand_rotation,
        readouts::build_watch_canvas(&readouts::BioReadout::from_world(world)),
    );
    scene_objects.append(&mut wrist_canvas(
        asset_cache,
        right_hand_position,
        right_hand_rotation,
        ammo_panel::build_wrist_canvas(&ammo_panel::AmmoReadout::from_world(world, false)),
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

/// One wrist canvas, hung on the hand. `readout` is whatever the SHARED
/// presentation-agnostic layout emitted - plate art included - so a wrist
/// cannot drift from the interface canvas or the flat HUD (AGENTS.md section
/// 3). Both wrists composite identically here, so a change to how one is hung
/// cannot miss the other.
///
/// No controls on either wrist: the VR pointer only hits the cyber-interface
/// panel, never these quads, so drawing a SETTING/RELOAD/cycle button nobody can
/// press would be a lie. Only the interactive layer differs, and it differs by
/// whether a pointer can reach it, not by presentation.
fn wrist_canvas(
    asset_cache: &mut AssetCache,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    readout: UiCanvas,
) -> Vec<SceneObject> {
    if readout.element_count() == 0 {
        return Vec::new();
    }
    readout.render_world_space(
        asset_cache,
        wrist_panel_transform(hand_position, hand_rotation, readout.size()),
        None,
        None,
        OVERLAY_Z_OFFSET,
    )
}

/// Where a wrist canvas of `canvas_size` pixels hangs: at [`WRIST_OFFSET`] in
/// the hand's frame, lying on the back of the wrist and facing out of it.
///
/// One rotation, no compensation: a -90 degree turn about the hand's X lays the
/// canvas on the back of the wrist. The basis stays honest (vr-ui-design rule
/// 4) - panel +Z, the face the viewer sees, is the hand's +Y (out of the back of
/// the hand); canvas up is panel +Y, which is the hand's -Z, pointing at the
/// fingers and so away from the player; canvas right follows the hand's +X.
/// That is the pose a watch is read in on a raised wrist.
///
/// The same offset and basis serve both hands: the glove model is mirrored for
/// the left hand, but the hand *frame* is not, so the back of the wrist is +Y
/// on both.
fn wrist_panel_transform(
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    canvas_size: Vector2<f32>,
) -> Matrix4<f32> {
    let position = hand_position + hand_rotation.rotate_vector(WRIST_OFFSET);
    let height = WRIST_CANVAS_WIDTH * canvas_size.y / canvas_size.x;
    Matrix4::from_translation(position)
        * Matrix4::from(hand_rotation)
        * Matrix4::from_angle_x(Deg(-90.0))
        * Matrix4::from_nonuniform_scale(WRIST_CANVAS_WIDTH, height, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::assets::asset_paths::AssetPath;

    use cgmath::{InnerSpace, One, Transform, Vector4, vec2, vec4};

    /// With the cyber interface up, the wrists emit nothing at all - the
    /// interface canvas is the single copy of both readouts (issue #1268).
    ///
    /// The gate short-circuits before the world or the asset cache is touched,
    /// which is what makes an empty `World` (no `PlayerInfo`) and an asset path
    /// that resolves nothing a sufficient fixture: an ungated build reaches
    /// both and fails.
    #[test]
    fn the_wrists_go_quiet_while_use_mode_is_up() {
        // No mounts: every lookup misses, so touching the cache is the failure
        // this test is looking for.
        let mut assets = AssetCache::new(String::new(), AssetPath::combine(vec![]));
        let objects = create_wrist_hud_panels(
            &mut assets,
            &World::new(),
            true,
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
        );
        assert!(objects.is_empty());
    }

    fn axis(transform: Matrix4<f32>, local: Vector4<f32>) -> Vector3<f32> {
        (transform * local).truncate().normalize()
    }

    /// The plate lies on the back of the wrist, facing out of it, reading
    /// toward the fingers - the pose a player checks a watch in. An identity
    /// hand frame makes the hand's axes the world's, so the expected vectors
    /// are just the frame's own.
    #[test]
    fn a_wrist_plate_faces_out_of_the_back_of_the_hand() {
        let transform = wrist_panel_transform(
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            readouts::WATCH_CROP_SIZE,
        );
        // Panel +Z (the face the viewer sees) is the back of the hand; canvas
        // up (panel +Y) points down the hand's aim direction at the fingers;
        // canvas right is the hand's own right.
        assert!(
            (axis(transform, vec4(0.0, 0.0, 1.0, 0.0)) - vec3(0.0, 1.0, 0.0)).magnitude() < 1e-5
        );
        assert!(
            (axis(transform, vec4(0.0, 1.0, 0.0, 0.0)) - vec3(0.0, 0.0, -1.0)).magnitude() < 1e-5
        );
        assert!(
            (axis(transform, vec4(1.0, 0.0, 0.0, 0.0)) - vec3(1.0, 0.0, 0.0)).magnitude() < 1e-5
        );
        // ...at the offset, toward the elbow and above the wrist.
        let centre = transform.transform_point(cgmath::point3(0.0, 0.0, 0.0));
        assert!((cgmath::vec3(centre.x, centre.y, centre.z) - WRIST_OFFSET).magnitude() < 1e-5);
    }

    /// Every wrist canvas is the same width and never stretched, so the two
    /// read as a matched pair of instruments however tall their art is.
    #[test]
    fn wrist_canvases_share_a_width_and_keep_their_aspect() {
        for size in [readouts::WATCH_CROP_SIZE, ammo_panel::WRIST_CROP_SIZE] {
            let transform = wrist_panel_transform(vec3(0.0, 0.0, 0.0), Quaternion::one(), size);
            let width = (transform * vec4(1.0, 0.0, 0.0, 0.0))
                .truncate()
                .magnitude();
            let height = (transform * vec4(0.0, 1.0, 0.0, 0.0))
                .truncate()
                .magnitude();
            assert!((width - WRIST_CANVAS_WIDTH).abs() < 1e-6, "{size:?}");
            assert!((height / width - size.y / size.x).abs() < 1e-6, "{size:?}");
        }
    }

    /// A wrist with nothing to say hangs nothing at all - an empty canvas must
    /// not become a bare plate strapped to the arm.
    #[test]
    fn an_empty_canvas_hangs_nothing() {
        let mut assets = AssetCache::new(String::new(), AssetPath::combine(vec![]));
        let objects = wrist_canvas(
            &mut assets,
            vec3(0.0, 0.0, 0.0),
            Quaternion::one(),
            UiCanvas::new(vec2(90.0, 44.0)),
        );
        assert!(objects.is_empty());
    }
}
