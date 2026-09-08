use cgmath::{Deg, Matrix4, vec3};
use dark::properties::{PropHitPoints, PropMaxHitPoints, PropPsiState};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Get, UniqueView, View, World};

use crate::{
    hud::{ammo_panel, readouts},
    mission::PlayerInfo,
    vr_config::Handedness,
};

/// Compact readouts ride the calibrated visible glove, not the raw controller.
/// Both wrists show health/psi dorsally; each underside shows only its own weapon.
pub fn create_wrist_hud_panels(
    asset_cache: &mut AssetCache,
    world: &World,
    use_mode: bool,
    poses: [crate::vr_support::GripPose; 2],
    wrist_frames: [Matrix4<f32>; 2],
) -> Vec<SceneObject> {
    if use_mode {
        return Vec::new();
    }
    let bio = readouts::BioReadout::from_world(world);
    let mut objects = Vec::new();
    for (i, hand) in [Handedness::Left, Handedness::Right]
        .into_iter()
        .enumerate()
    {
        if !poses[i].is_tracked()
            || !crate::virtual_hand::shows_hand_visual(
                world,
                crate::wielded_weapon::held_by_hand(world, hand),
            )
        {
            continue;
        }
        let root = Matrix4::from_translation(poses[i].position)
            * Matrix4::from(poses[i].rotation)
            * wrist_frames[i];
        for (under, canvas) in [
            (false, readouts::build_watch_canvas(&bio)),
            (
                true,
                ammo_panel::build_wrist_canvas(&ammo_panel::AmmoReadout::for_weapon(
                    world,
                    crate::wielded_weapon::weapon_in_hand(world, hand),
                    false,
                )),
            ),
        ] {
            if canvas.element_count() == 0 {
                continue;
            }
            objects.extend(canvas.render_world_space(
                asset_cache,
                wrist_panel_transform(root, canvas.size(), under, hand),
                None,
                None,
                0.001,
            ));
        }
    }
    crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
    objects
}

/// Wrist-frame +Z points out of the glove's back; +Y points toward its fingers.
/// The underside turns around +Y so glyphs remain readable, never mirrored.
fn wrist_panel_transform(
    root: Matrix4<f32>,
    canvas_size: cgmath::Vector2<f32>,
    under: bool,
    hand: Handedness,
) -> Matrix4<f32> {
    const WIDTH: f32 = 0.085;
    // The bio face runs around the wrist like a bracelet. Keep the ammo
    // face oriented for an across-body underside glance.
    let roll = if !under {
        0.0
    } else if hand == Handedness::Right {
        -90.0
    } else {
        90.0
    };
    root * Matrix4::from_translation(vec3(0.0, -0.015, if under { -0.06 } else { 0.04 }))
        * Matrix4::from_angle_y(Deg(if under { 180.0 } else { 0.0 }))
        * Matrix4::from_angle_z(Deg(roll))
        * Matrix4::from_nonuniform_scale(WIDTH, WIDTH * canvas_size.y / canvas_size.x, 1.0)
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
/// `(discipline name, tier)`. `Some` only while the supplied weapon is the
/// psi amp (class tag `weapontype psiamp`).
pub(crate) fn get_weapon_psi_power(
    world: &World,
    weapon: Option<shipyard::EntityId>,
) -> Option<(String, i32)> {
    // The psi amp is resolved via its template's class tags, the same mechanism
    // as `get_weapon_ammo_type`.
    if !crate::wielded_weapon::is_psi_amp(world, weapon?) {
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

/// Current clip ammo of this weapon, or `None` for an empty hand or an item
/// without `PropGunState` (melee / unlimited debug weapons).
pub(crate) fn get_weapon_ammo(world: &World, weapon: Option<shipyard::EntityId>) -> Option<i32> {
    let weapon = weapon?;
    let v_gun_state = world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()?;
    v_gun_state.get(weapon).ok().map(|g| g.ammo)
}

/// The template id of this weapon's currently selected `Projectile` link
/// (its ammo type), or `None` when unarmed or the weapon has no projectile links
/// (melee). Honors `RuntimePropSelectedAmmo` (absent = the first link).
pub(crate) fn weapon_selected_projectile_template(
    world: &World,
    weapon: Option<shipyard::EntityId>,
) -> Option<i32> {
    let weapon = weapon?;
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

/// The `ammotype` class-tag of this weapon's selected ammo (e.g. "std",
/// "he", "ap"), or `None`.
pub(crate) fn get_weapon_ammo_type(
    world: &World,
    weapon: Option<shipyard::EntityId>,
) -> Option<String> {
    let template_id = weapon_selected_projectile_template(world, weapon)?;
    let class_tags = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateClassTags>>()
        .ok()?;
    class_tags.0.get(&template_id)?.get("ammotype").cloned()
}

/// This gun's fire setting: its index (0 or 1) and the short header for
/// that setting ("NORM" / "BURST"), or `None` when nothing gun-like is wielded.
/// The header is absent for a gun whose data names no header for the setting.
pub(crate) fn get_weapon_gun_setting(
    world: &World,
    weapon: Option<shipyard::EntityId>,
) -> Option<(i32, Option<String>)> {
    let weapon = weapon?;
    let setting = world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.setting))?;
    let header = crate::scripts::script_util::gun_setting_header(world, weapon, setting);
    Some((setting, header))
}

/// The object-icon bitmap filename (e.g. "STD_I.pcx") of this weapon's
/// selected ammo type, or `None`. Resolved from the selected projectile
/// template's `P$ObjIcon` (projectiles are templates, not instantiated entities,
/// so this reads the precomputed [`GlobalTemplateObjIcons`] map rather than a
/// `View`).
pub(crate) fn get_weapon_ammo_icon(
    world: &World,
    weapon: Option<shipyard::EntityId>,
) -> Option<String> {
    let template_id = weapon_selected_projectile_template(world, weapon)?;
    let icons = world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateObjIcons>>()
        .ok()?;
    icons.0.get(&template_id).cloned()
}

// Hand-agnostic snapshots preserve the existing dominant-weapon convention.
pub(crate) fn get_wielded_ammo_type(world: &World) -> Option<String> {
    get_weapon_ammo_type(world, crate::wielded_weapon::wielded_weapon(world))
}
pub(crate) fn get_wielded_gun_setting(world: &World) -> Option<(i32, Option<String>)> {
    get_weapon_gun_setting(world, crate::wielded_weapon::wielded_weapon(world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, SquareMatrix};

    #[test]
    fn opposite_watch_faces_are_outward_and_text_is_never_mirrored() {
        for (under, hand) in [
            (false, Handedness::Left),
            (true, Handedness::Left),
            (false, Handedness::Right),
            (true, Handedness::Right),
        ] {
            let transform =
                wrist_panel_transform(Matrix4::identity(), cgmath::vec2(128.0, 44.0), under, hand);
            let normal = transform.z.truncate();
            assert!(normal.dot(transform.w.truncate()) > 0.0);
            assert!(transform.determinant() > 0.0);
            assert!(
                (transform.x.truncate().magnitude() / transform.y.truncate().magnitude()
                    - 128.0 / 44.0)
                    .abs()
                    < 0.0001
            );
        }
    }
}
