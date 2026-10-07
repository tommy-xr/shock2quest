use cgmath::{Deg, Matrix4, vec3};
use dark::properties::{PropGunState, PropHitPoints, PropMaxHitPoints, PropPsiState};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Get, UniqueView, View, World};

use crate::{
    hud::{ammo_panel, readouts},
    mission::PlayerInfo,
    vr_config::Handedness,
};

/// Compact readouts ride the calibrated visible glove, not the raw controller;
/// a held psi amp carries the bio band on its own baked wrist instead.
/// Both wrists show health/psi, back and palm; each cuff opening shows its own weapon, or the
/// weapon it steadies (`supported`, per hand) - with a two-handed gun the
/// support wrist is often the only one facing the player.
pub fn create_wrist_hud_panels(
    asset_cache: &mut AssetCache,
    world: &World,
    use_mode: bool,
    poses: [crate::vr_support::GripPose; 2],
    wrist_frames: [Matrix4<f32>; 2],
    supported: [Option<shipyard::EntityId>; 2],
) -> Vec<SceneObject> {
    if use_mode {
        return Vec::new();
    }
    let bio = readouts::BioReadout::from_world(world);
    let alarm = crate::security_alarm::status(world).hud_seconds();
    let mut objects = Vec::new();
    for (i, hand) in [Handedness::Left, Handedness::Right]
        .into_iter()
        .enumerate()
    {
        if !poses[i].is_tracked() {
            continue;
        }
        let root = Matrix4::from_translation(poses[i].position)
            * Matrix4::from(poses[i].rotation)
            * wrist_frames[i];
        if hand == Handedness::Left {
            let alarm_canvas = alarm
                .map(super::alarm_panel::build_panel_canvas)
                .unwrap_or_else(|| crate::ui::UiCanvas::new(super::alarm_panel::PANEL));
            for (canvas, offset_x) in [
                (
                    super::breath::wrist_canvas(crate::swimming::WaterStatus::from_world(world)),
                    -0.17,
                ),
                (alarm_canvas, 0.0),
            ] {
                if canvas.element_count() == 0 {
                    continue;
                }
                objects.extend(canvas.render_world_space(
                    asset_cache,
                    wrist_hologram_transform(root, canvas.size(), offset_x),
                    None,
                    None,
                    0.001,
                ));
            }
        }
        if let Some(notice) = crate::wielded_weapon::held_by_hand(world, hand).and_then(|weapon| {
            crate::weapon_requirements::active_weapon_skill_notice(world, weapon)
        }) {
            let font = crate::ui::resolve_font(asset_cache, crate::hud::message_line::FONT);
            let canvas = crate::hud::message_line::build_message_canvas(
                &[notice.message()],
                font.as_ref().as_ref(),
            );
            // Same pixel layout as the flat status line. Only the panel's
            // world placement differs: above this glove.
            let width = 0.32;
            // Leave the full hazard panel unobscured when a left-hand weapon
            // also needs to explain a skill refusal.
            let notice_y = if hand == Handedness::Left && alarm.is_some() {
                0.17
            } else {
                0.06
            };
            let transform = root
                * Matrix4::from_translation(vec3(0.0, notice_y, 0.10))
                * Matrix4::from_nonuniform_scale(
                    width,
                    width * canvas.size().y / canvas.size().x,
                    1.0,
                );
            objects.extend(canvas.render_world_space(asset_cache, transform, None, None, 0.001));
        }
        // Some weapons carry an authored hand instead of our glove. Give a
        // melee charge a hologram mount; there is no physical cuff to host it.
        // Only a steadied gun lends its readout: a melee weapon's charge
        // already shows on the hand swinging it.
        let steadied_gun = supported[i].filter(|&entity| {
            world
                .borrow::<View<PropGunState>>()
                .is_ok_and(|guns| guns.contains(entity))
        });
        let held = crate::wielded_weapon::held_by_hand(world, hand);
        let weapon = held.or(steadied_gun);
        let readout = ammo_panel::AmmoReadout::for_weapon(world, weapon, false);
        let bio_canvas = readouts::build_watch_canvas(&bio);
        // The refusal is available even when a weapon carries its own hand
        // mesh. The cuff's wrist plates require a visible glove.
        if !crate::virtual_hand::shows_hand_visual(world, held) {
            if readout.melee_charge.is_some() {
                let canvas = ammo_panel::build_wrist_canvas(&readout);
                objects.extend(canvas.render_world_space(
                    asset_cache,
                    authored_weapon_readout_transform(
                        Matrix4::from_translation(poses[i].position)
                            * Matrix4::from(poses[i].rotation),
                        canvas.size(),
                    ),
                    None,
                    None,
                    0.001,
                ));
            }
            // The amp's baked forearm is thicker than the glove's cuff: scale
            // the same band out to it.
            if let Some((wrist, radius)) = held
                .filter(|&amp| crate::wielded_weapon::is_psi_amp(world, amp))
                .and_then(|amp| crate::psi_amp_readout::wrist(world, amp, hand))
            {
                let root = wrist * Matrix4::from_scale(radius / BIO_BAND_RADIUS);
                objects.extend(bio_bands(asset_cache, &bio_canvas, root, 0.0));
            }
            continue;
        }
        objects.extend(bio_bands(asset_cache, &bio_canvas, root, GLOVE_PALM_LIFT));
        let ammo_canvas = ammo_panel::build_wrist_canvas(&readout);
        // A psi amp carries its own readout (`psi_amp_readout`).
        if ammo_canvas.element_count() > 0 && readout.psi_power.is_none() {
            objects.extend(ammo_canvas.render_world_space(
                asset_cache,
                wrist_panel_transform(root, ammo_canvas.size(), true),
                None,
                None,
                0.001,
            ));
        }
    }
    crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
    objects
}

/// The bio band on both sides of the wrist, back and palm, so it faces the
/// player whichever way the hand is turned. `palm_lift` moves the palm band
/// out past a cuff that is thicker on that side.
fn bio_bands(
    asset_cache: &mut AssetCache,
    canvas: &crate::ui::UiCanvas,
    root: Matrix4<f32>,
    palm_lift: f32,
) -> Vec<SceneObject> {
    bio_band_transforms(root, canvas.size(), palm_lift)
        .into_iter()
        .flat_map(|transform| {
            canvas.render_world_space_bent(
                asset_cache,
                transform,
                BIO_BAND_RADIUS / BIO_WIDTH,
                0.001 / BIO_WIDTH,
            )
        })
        .collect()
}

fn bio_band_transforms(
    root: Matrix4<f32>,
    canvas_size: cgmath::Vector2<f32>,
    palm_lift: f32,
) -> [Matrix4<f32>; 2] {
    [(Deg(0.0), 0.0), (Deg(180.0), palm_lift)].map(|(side, lift)| {
        let side =
            root * Matrix4::from_angle_y(side) * Matrix4::from_translation(vec3(0.0, 0.0, lift));
        wrist_panel_transform(side, canvas_size, false)
    })
}

/// An authored weapon hand mesh has no glove cuff. Lift the unchanged shared
/// readout above its back in the final controller pose, without a glove basis.
fn authored_weapon_readout_transform(
    root: Matrix4<f32>,
    size: cgmath::Vector2<f32>,
) -> Matrix4<f32> {
    // The authored fist has no calibrated glove wrist. Use its final hand
    // pose directly: +Y clears the weapon, and +Z faces back toward the player.
    let width = 0.24;
    root * Matrix4::from_translation(vec3(0.0, 0.55, 0.25))
        * Matrix4::from_angle_x(Deg(-20.0))
        * Matrix4::from_nonuniform_scale(width, width * size.y / size.x, 1.0)
}

/// Shared lower-edge hinge for the hazard and alarm canvases. A canvas pixel
/// occupies 1.25 mm, preserving the approved 16 cm hazard width. Positive X
/// tilt lifts the top away from the glove; health/psi remain on the bracelet.
fn wrist_hologram_transform(
    root: Matrix4<f32>,
    size: cgmath::Vector2<f32>,
    offset_x: f32,
) -> Matrix4<f32> {
    let scale = 0.16 / 128.0;
    let height = size.y * scale;
    root * Matrix4::from_translation(vec3(offset_x, 0.02375, 0.08))
        * Matrix4::from_angle_x(Deg(45.0))
        * Matrix4::from_translation(vec3(0.0, height * 0.5, 0.0))
        * Matrix4::from_nonuniform_scale(size.x * scale, height, 1.0)
}

const BIO_WIDTH: f32 = 0.085;
/// The bio band wraps a cylinder about the wrist axis, just outside the cuff.
pub(crate) const BIO_BAND_RADIUS: f32 = 0.045;
/// The glove's cuff stands further off the wrist on the palm side.
pub(crate) const GLOVE_PALM_LIFT: f32 = 0.02;

/// Pinky side of the same bracelet. Rendering and grab
/// targeting share this frame, including the thicker palm side of the glove.
pub(crate) fn implant_socket_frame(
    root: Matrix4<f32>,
    hand: Handedness,
    palm_lift: f32,
) -> Matrix4<f32> {
    let side = if hand == Handedness::Left { -1.0 } else { 1.0 };
    root * Matrix4::from_translation(vec3(
        side * (BIO_BAND_RADIUS + 0.008),
        -0.015,
        -palm_lift * 0.5,
    )) * Matrix4::from_angle_y(Deg(side * 90.0))
}

pub(crate) const IMPLANT_SLOT_WIDTH: f32 = BIO_WIDTH * 34.0 / 128.0 * 1.75;

/// Wrist-frame +Z points out of the glove's back; +Y points toward its fingers.
/// Bio faces dorsally, scaled alike in x and z so it can bend around the
/// wrist. Ammo caps the cuff opening and faces back along the forearm,
/// rolled in its face plane so its text stays upright.
fn wrist_panel_transform(
    root: Matrix4<f32>,
    canvas_size: cgmath::Vector2<f32>,
    ammo: bool,
) -> Matrix4<f32> {
    let width = if ammo { 0.058 } else { BIO_WIDTH };
    let placed = if ammo {
        roll_upright(
            root * Matrix4::from_translation(vec3(0.0, -0.026, -0.005))
                * Matrix4::from_angle_x(Deg(90.0)),
        )
    } else {
        root * Matrix4::from_translation(vec3(0.0, -0.015, 0.04))
    };
    let depth = if ammo { 1.0 } else { width };
    placed * Matrix4::from_nonuniform_scale(width, width * canvas_size.y / canvas_size.x, depth)
}

/// Keep `m`'s origin and facing (+Z), but roll about that facing so +Y is as
/// close to world up as it can be. Near a vertical facing "up" is undefined
/// and would spin with small sway, so the correction fades back to `m`'s own
/// roll there (e.g. an arm hanging at the side).
fn roll_upright(m: Matrix4<f32>) -> Matrix4<f32> {
    use cgmath::InnerSpace;
    let normal = m.z.truncate().normalize();
    let own = m.y.truncate().normalize();
    let upright = cgmath::Vector3::unit_y() - normal * normal.y;
    if upright.magnitude2() < 1e-6 {
        return m;
    }
    let upright = upright.normalize();
    let t = ((normal.y.abs() - 0.85) / (0.97 - 0.85)).clamp(0.0, 1.0);
    let keep = t * t * (3.0 - 2.0 * t);
    let angle = normal.dot(own.cross(upright)).atan2(own.dot(upright)) * (1.0 - keep);
    let up = own * angle.cos() + normal.cross(own) * angle.sin();
    let right = up.cross(normal);
    Matrix4::from_cols(right.extend(0.0), up.extend(0.0), normal.extend(0.0), m.w)
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

    let power = crate::psi_amp_selection::selected_power(world, weapon?)?;
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
    fn holograms_share_a_lower_hinge_and_lift_at_45_degrees() {
        for size in [super::super::alarm_panel::PANEL] {
            let transform = wrist_hologram_transform(Matrix4::identity(), size, 0.0);
            let lower = transform * cgmath::vec4(0.0, -0.5, 0.0, 1.0);
            let upper = transform * cgmath::vec4(0.0, 0.5, 0.0, 1.0);
            assert!((lower.y - 0.02375).abs() < 0.00001);
            assert!((lower.z - 0.08).abs() < 0.00001);
            assert!(upper.z > lower.z);
            assert!(((upper.y - lower.y) - (upper.z - lower.z)).abs() < 0.00001);
            assert!(transform.determinant() > 0.0);
        }
    }

    #[test]
    fn authored_weapon_mount_preserves_the_canvas_aspect_and_reading_direction() {
        let size = cgmath::vec2(94.0, 64.0);
        let transform = authored_weapon_readout_transform(Matrix4::identity(), size);
        assert!(transform.determinant() > 0.0);
        assert!(
            (transform.x.truncate().magnitude() / transform.y.truncate().magnitude()
                - size.x / size.y)
                .abs()
                < 0.0001
        );
        let lower = transform * cgmath::vec4(0.0, -0.5, 0.0, 1.0);
        assert!(
            lower.z > 0.13,
            "the readout clears the authored hand instead of occupying a nonexistent cuff"
        );
    }

    #[test]
    fn bio_bands_face_out_of_both_sides_of_the_wrist() {
        let [back, palm] =
            bio_band_transforms(Matrix4::identity(), cgmath::vec2(128.0, 44.0), 0.02);
        for band in [back, palm] {
            // Outward, unmirrored, and reading toward the fingers on both sides.
            assert!(band.z.truncate().dot(band.w.truncate()) > 0.0);
            assert!(band.determinant() > 0.0);
            assert!(band.y.truncate().normalize().dot(vec3(0.0, 1.0, 0.0)) > 0.999);
        }
        assert!(back.z.z > 0.0 && palm.z.z < 0.0);
        assert!((-palm.w.z - back.w.z - 0.02).abs() < 1e-6);
    }

    #[test]
    fn glove_readouts_are_outward_and_text_is_never_mirrored() {
        for ammo in [false, true] {
            let transform =
                wrist_panel_transform(Matrix4::identity(), cgmath::vec2(128.0, 44.0), ammo);
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

    #[test]
    fn ammo_readout_stays_upright_however_the_wrist_rolls() {
        let size = cgmath::vec2(94.0, 64.0);
        for roll in [0.0, 37.0, 90.0, 180.0, 250.0] {
            // A forearm pointing forward (-Z), rolled about itself.
            let root = Matrix4::from_angle_z(Deg(roll)) * Matrix4::from_angle_x(Deg(-90.0));
            let transform = wrist_panel_transform(root, size, true);
            let up = transform.y.truncate().normalize();
            assert!(up.y > 0.999, "roll {roll}: up {up:?}");
            assert!(transform.determinant() > 0.0);
        }
    }

    #[test]
    fn ammo_readout_does_not_spin_when_the_forearm_passes_vertical() {
        let size = cgmath::vec2(94.0, 64.0);
        // Swing a rolled forearm up through vertical in small steps; the
        // readout's up must move smoothly, not flip.
        let up_at = |pitch: f32| {
            let root = Matrix4::from_angle_x(Deg(pitch))
                * Matrix4::from_angle_z(Deg(30.0))
                * Matrix4::from_angle_x(Deg(-90.0));
            let transform = wrist_panel_transform(root, size, true);
            assert!(transform.determinant() > 0.0);
            transform.y.truncate().normalize()
        };
        for step in 0..200 {
            let pitch = 40.0 + step as f32 * 0.5;
            assert!(
                up_at(pitch).dot(up_at(pitch + 0.5)) > 0.99,
                "pitch {pitch}: jumped"
            );
        }
    }
}
