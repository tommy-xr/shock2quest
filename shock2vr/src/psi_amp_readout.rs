//! VR readout carried by a wielded psi amp: the selected power's hologram
//! floats over the sphere, and the overload meter wraps around it. Both follow
//! the amp's rendered transform, so a rescaled amp keeps them on its surface.
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix3, Matrix4, Point3, Rad, Transform, Vector3, vec2, vec3,
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    Handedness,
    runtime_props::PsiChargePhase,
    ui::{Rect, UiCanvas},
};

/// The ball of `amp_h.bin` in model space: a least-squares sphere fit to its
/// surface vertices near the ball. The model also carries a cable, a baked
/// hand and a forearm, so its bounds do not locate the ball. The 25AE and
/// classic meshes fit within 0.02 of these.
const AMP_SPHERE_CENTER: Point3<f32> = Point3::new(-0.42, 0.094, -0.04);
const AMP_SPHERE_RADIUS: f32 = 0.155;
/// The baked forearm's axis at the wrist, in model space; the forearm runs
/// along model +x. Mount toward the elbow so the pinky-side implant well
/// clears the authored cable inlet. This frame drives both bio bands and the socket.
const AMP_WRIST_CENTER: Point3<f32> = Point3::new(0.08, -0.12, -0.10);
/// Bands at this radius sit just off the forearm's skin, palm and back.
const AMP_FOREARM_RADIUS: f32 = 0.15;
const ICON_SIZE: f32 = 0.05;
/// Minimum gap between the sphere's top and the badge's bottom edge.
const ICON_GAP: f32 = 0.04;
/// Extra badge gap with the hand over the ball (see [`badge_lift`]).
const FINGER_CLEARANCE: f32 = 0.03;
/// The meter's clearance off the sphere, in sphere radii.
const RING_CLEARANCE: f32 = 0.2;
const RING_ARC: Deg<f32> = Deg(80.0);
const METER: Rect = Rect::new(0.0, 0.0, 60.0, 19.0);
const PIP_ON: [u8; 3] = [120, 255, 220];
const PIP_OFF: [u8; 3] = [40, 90, 80];
/// Bent around the ball, LOADBACK is nearly the fill's green; dimmed over black
/// (the band stays opaque), the charge reads at a glance. Flat keeps the retail art.
const METER_BACK_OPACITY: f32 = 0.35;

pub(crate) fn render(
    world: &World,
    assets: &mut AssetCache,
    eye: Vector3<f32>,
    except: Option<EntityId>,
) -> Vec<SceneObject> {
    let mut objects = Vec::new();
    for hand in [Handedness::Left, Handedness::Right] {
        let Some(amp) = crate::wielded_weapon::held_by_hand(world, hand)
            .filter(|&amp| Some(amp) != except && crate::wielded_weapon::is_psi_amp(world, amp))
        else {
            continue;
        };
        let Some((center, radius, axes)) = sphere(world, amp, hand) else {
            continue;
        };
        let readout = crate::hud::ammo_panel::AmmoReadout::for_weapon(world, Some(amp), false);
        if let Some((fraction, phase)) = readout.psi_charge {
            let canvas = meter_canvas(fraction, phase);
            let transform = ring_transform(center, radius, axes.y, axes.x);
            let width = transform.x.magnitude();
            objects.extend(canvas.render_world_space_bent(
                assets,
                transform,
                ring_radius(radius) / width,
                0.001 / width,
            ));
        }
        if let Some(power) = crate::psi_amp_selection::selected_power(world, amp) {
            let strings = world
                .borrow::<UniqueView<crate::scripts::gui::GlobalPsiStrings>>()
                .ok();
            let empty = std::collections::HashMap::new();
            let strings = strings.as_ref().map_or(&empty, |s| &s.0);
            let icon = crate::scripts::gui::icon_texture(
                &crate::scripts::gui::icon_basename(strings, power.power.power_id),
                1,
            );
            let canvas = badge_canvas(&icon, power.tier());
            let height = ICON_SIZE * canvas.size().y / canvas.size().x;
            let origin = center + vec3(0.0, radius + badge_lift(axes.y) + height * 0.5, 0.0);
            let transform = crate::psi_carousel::projection_frame(origin, eye)
                * Matrix4::from_nonuniform_scale(ICON_SIZE, height, 1.0);
            objects.extend(canvas.render_world_space(assets, transform, None, None, 0.001));
        }
    }
    crate::util::tag_render_source(&mut objects, crate::util::render_source::PLAYER_HANDS);
    objects
}

/// The selected power's luminous icon over a row of five tier pips.
fn badge_canvas(icon: &str, tier: i32) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(48.0, 58.0));
    crate::psi_carousel::luminous_icon(&mut canvas, Rect::new(3.0, 3.0, 42.0, 42.0), icon, 1.0);
    for pip in 0..5 {
        let lit = pip < tier;
        canvas
            .fill(
                Rect::new(3.0 + pip as f32 * 9.0, 50.0, 6.0, 4.0),
                if lit { PIP_ON } else { PIP_OFF },
            )
            .opacity(if lit { 0.95 } else { 0.5 });
    }
    canvas
}

/// The retail overload meter art, the same states as the flat ammo panel,
/// with its background dimmed.
fn meter_canvas(fraction: f32, phase: PsiChargePhase) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(METER.w, METER.h));
    match phase {
        PsiChargePhase::Charging => {
            canvas
                .fill(METER, [0, 0, 0])
                .image(METER, "LOADBACK.PCX")
                .opacity(METER_BACK_OPACITY)
                .bar(METER, "LOADMETR.PCX", fraction);
        }
        PsiChargePhase::Overloaded => {
            canvas.image(METER, "LOADGOOD.PCX");
        }
        PsiChargePhase::Burnout => {
            canvas.image(METER, "LOADBURN.PCX");
        }
    }
    canvas
}

/// World centre, radius and unit model axes of the amp's sphere, composed
/// exactly as the held amp mesh is drawn (charge offset, pose, fit scale,
/// left-hand mirror). Model +y runs palm to ball; +x points at the wrist.
pub(crate) fn sphere(
    world: &World,
    amp: EntityId,
    hand: Handedness,
) -> Option<(Vector3<f32>, f32, Matrix3<f32>)> {
    let transform = drawn_transform(world, amp, hand)?;
    Some((
        transform.transform_point(AMP_SPHERE_CENTER).to_vec(),
        AMP_SPHERE_RADIUS * transform.x.truncate().magnitude(),
        Matrix3::from_cols(
            transform.x.truncate().normalize(),
            transform.y.truncate().normalize(),
            transform.z.truncate().normalize(),
        ),
    ))
}

/// Gap under the badge for a palm-to-ball axis `up`: least palm up, most once
/// the fingers (palm sideways) or the whole hand (palm down) are over the top.
fn badge_lift(up: Vector3<f32>) -> f32 {
    ICON_GAP + FINGER_CLEARANCE * (1.0 - up.y.max(0.0))
}

/// The amp's baked wrist as a glove-style wrist frame (+Y toward the fingers,
/// +Z out of the back of the hand, unit and right-handed in either hand), with
/// the forearm's world radius.
pub(crate) fn wrist(world: &World, amp: EntityId, hand: Handedness) -> Option<(Matrix4<f32>, f32)> {
    let transform = drawn_transform(world, amp, hand)?;
    Some((
        wrist_frame(transform),
        AMP_FOREARM_RADIUS * transform.x.truncate().magnitude(),
    ))
}

fn wrist_frame(transform: Matrix4<f32>) -> Matrix4<f32> {
    let fingers = -transform.x.truncate().normalize();
    let back = -transform.y.truncate().normalize();
    Matrix4::from_cols(
        fingers.cross(back).extend(0.0),
        fingers.extend(0.0),
        back.extend(0.0),
        transform.transform_point(AMP_WRIST_CENTER).to_homogeneous(),
    )
}

/// The amp's model-to-world transform, as its mesh is drawn (charge offset,
/// pose, fit scale, left-hand mirror).
fn drawn_transform(world: &World, amp: EntityId, hand: Handedness) -> Option<Matrix4<f32>> {
    let transforms = world
        .borrow::<View<crate::runtime_props::RuntimePropTransform>>()
        .ok()?;
    Some(
        crate::melee_charge_visual::transform(world, Some(amp))
            * transforms.get(amp).ok()?.0
            * crate::vr_config::psi_amp_fit_scale()
            * hand.gun_mirror(),
    )
}

fn ring_radius(sphere_radius: f32) -> f32 {
    sphere_radius * (1.0 + RING_CLEARANCE)
}

/// Bend the meter around the amp's palm-to-ball axis, so it rolls with the
/// sphere (level with the palm up, upright with the palm sideways), its middle
/// pinned to the wrist side, which faces the player and is clear of the
/// fingers at any roll.
fn ring_transform(
    center: Vector3<f32>,
    sphere_radius: f32,
    up: Vector3<f32>,
    wrist: Vector3<f32>,
) -> Matrix4<f32> {
    let normal = (wrist - up * wrist.dot(up)).normalize();
    let radius = ring_radius(sphere_radius);
    let width = Rad::from(RING_ARC).0 * radius;
    let height = width * METER.h / METER.w;
    Matrix4::from_cols(
        up.cross(normal).extend(0.0),
        up.extend(0.0),
        normal.extend(0.0),
        (center + normal * radius).extend(1.0),
    ) * Matrix4::from_nonuniform_scale(width, height, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::SquareMatrix;

    #[test]
    fn ring_wraps_the_palm_axis_facing_the_wrist() {
        let center = vec3(1.0, 2.0, 3.0);
        // Palm sideways: the amp's up axis is world +x.
        let up = vec3(1.0, 0.0, 0.0);
        let wrist = vec3(0.5, 0.3, 0.4);
        let ring = ring_transform(center, 0.1, up, wrist);
        assert!(ring.determinant() > 0.0);
        // Panel up follows the palm axis, so the band stands upright.
        assert!(ring.y.truncate().normalize().dot(up) > 0.999);
        assert!(ring.x.truncate().dot(up).abs() < 1e-6);
        // Its middle faces the wrist, projected off that axis.
        let normal = ring.z.truncate().normalize();
        assert!(normal.dot(vec3(0.0, 0.6, 0.8)) > 0.999);
        // The panel's middle sits just outside the sphere, on its equator.
        let offset = ring.w.truncate() - center;
        assert!(offset.dot(up).abs() < 1e-6);
        assert!((offset.magnitude() - ring_radius(0.1)).abs() < 1e-5);
        assert!(ring_radius(0.1) > 0.1);
        // Bending needs panel x and z scaled alike.
        assert!((ring.x.magnitude() - ring.z.magnitude()).abs() < 1e-6);
    }

    #[test]
    fn sphere_follows_the_left_hand_mirror_and_amp_roll() {
        let mut world = World::new();
        let amp = world.add_entity((crate::runtime_props::RuntimePropTransform(
            Matrix4::from_translation(vec3(1.0, 2.0, 3.0)) * Matrix4::from_angle_z(Deg(-90.0)),
        ),));
        let (right, radius, right_axes) = sphere(&world, amp, Handedness::Right).unwrap();
        let (left, _, left_axes) = sphere(&world, amp, Handedness::Left).unwrap();
        let scale = crate::vr_config::psi_amp_fit_scale().x.x;
        assert!((radius - AMP_SPHERE_RADIUS * scale).abs() < 1e-6);
        assert!((right.z - 3.0 - AMP_SPHERE_CENTER.z * scale).abs() < 1e-6);
        assert!((left.z - 3.0 + AMP_SPHERE_CENTER.z * scale).abs() < 1e-6);
        // Rolling the amp rolls its palm axis and wrist side, in either hand.
        for axes in [right_axes, left_axes] {
            assert!(axes.y.dot(vec3(1.0, 0.0, 0.0)) > 0.999);
            assert!(axes.x.dot(vec3(0.0, -1.0, 0.0)) > 0.999);
        }
    }

    #[test]
    fn badge_clears_the_hand_over_the_ball() {
        let up = badge_lift(vec3(0.0, 1.0, 0.0));
        let sideways = badge_lift(vec3(1.0, 0.0, 0.0));
        let down = badge_lift(vec3(0.0, -1.0, 0.0));
        assert!((up - ICON_GAP).abs() < 1e-6);
        assert!((sideways - ICON_GAP - FINGER_CLEARANCE).abs() < 1e-6);
        assert_eq!(down, sideways);
    }

    #[test]
    fn amp_wrist_is_a_glove_style_frame_in_either_hand() {
        let pose = Matrix4::from_translation(vec3(1.0, 2.0, 3.0))
            * Matrix4::from_angle_z(Deg(-90.0))
            * Matrix4::from_scale(0.4);
        for hand in [Handedness::Right, Handedness::Left] {
            let model = pose * hand.gun_mirror();
            let frame = wrist_frame(model);
            assert!((frame.determinant() - 1.0).abs() < 1e-5);
            // +Y toward the fingers (model -x), +Z out of the back (model -y).
            assert!(frame.y.truncate().dot(-model.x.truncate().normalize()) > 0.999);
            assert!(frame.z.truncate().dot(-model.y.truncate().normalize()) > 0.999);
            let centre = model.transform_point(AMP_WRIST_CENTER).to_vec();
            assert!((frame.w.truncate() - centre).magnitude() < 1e-6);
        }
    }

    #[test]
    fn badge_lights_one_pip_per_tier() {
        let lit = |tier| {
            badge_canvas("icon.pcx", tier)
                .elements()
                .iter()
                .filter(
                    |e| matches!(e, crate::ui::UiElement::Fill { color, .. } if *color == PIP_ON),
                )
                .count()
        };
        assert_eq!((lit(1), lit(3), lit(5)), (1, 3, 5));
    }
}
