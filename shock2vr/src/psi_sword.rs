//! The blade is an addition to the original amp, never a replacement item.
use cgmath::{InnerSpace, Matrix4, Point3, Quaternion, Transform, Vector3, vec3};
use dark::importers::WEAPON_SURFACE_IMPORTER;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Component, EntityId, UniqueView, View, World};
pub const POWER: i32 = -1119;
pub const WEAPON: i32 = -2291;
pub const LENGTH: f32 = 1.4;
#[derive(Component)]
pub struct BoundBlade;
pub fn active(world: &World, amp: EntityId) -> bool {
    world
        .borrow::<View<BoundBlade>>()
        .is_ok_and(|v| v.contains(amp))
        && world
            .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
            .is_ok_and(|p| p.is_active(POWER))
}
pub fn frame(world: &World, amp: EntityId) -> Option<Matrix4<f32>> {
    let hand = if crate::mission::presentation_is_vr(world) {
        Some(
            [crate::Handedness::Right, crate::Handedness::Left]
                .into_iter()
                .find(|&hand| crate::wielded_weapon::held_by_hand(world, hand) == Some(amp))?,
        )
    } else {
        None
    };
    let model = crate::psi_amp_readout::model_transform(world, amp, hand)?;
    // VR seats the blade out of the fist at the default amp roll; roll tuning
    // turns it with the amp. Flat stays upright. Fitting never scales reach.
    let right = model.x.truncate().normalize();
    let up = if hand.is_some() {
        -model.z.truncate().normalize()
    } else {
        model.y.truncate().normalize()
    };
    Some(Matrix4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        right.cross(up).extend(0.0),
        model
            .transform_point(crate::psi_amp_readout::AMP_SPHERE_CENTER)
            .to_homogeneous(),
    ))
}
pub fn segment(world: &World, amp: EntityId) -> Option<(Point3<f32>, Point3<f32>)> {
    let frame = frame(world, amp)?;
    Some((
        frame.transform_point(Point3::new(0.0, 0.0, 0.0)),
        frame.transform_point(Point3::new(0.0, LENGTH, 0.0)),
    ))
}
pub fn render(world: &World, cache: &mut AssetCache, amp: EntityId) -> Vec<SceneObject> {
    if !active(world, amp) {
        return vec![];
    }
    let Some(frame) = frame(world, amp) else {
        return vec![];
    };
    let Some(source) = cache.get_opt(&WEAPON_SURFACE_IMPORTER, "psword_h.bin") else {
        return vec![];
    };
    let Some(source) = source.as_ref().as_ref() else {
        return vec![];
    };
    let Some(local) = blade_transform(&source.triangles) else {
        return vec![];
    };
    let mut object = source.object.clone();
    object.set_transform(frame * local);
    object.replace_material(std::rc::Rc::new(std::cell::RefCell::new(
        engine::scene::color_material::create(vec3(0.12, 0.8, 1.0)),
    )));
    object.set_depth_write(false);
    object.set_transparency(Some(0.15));
    crate::psi_invisibility::apply(&mut object, crate::psi_invisibility::transparency(world));
    vec![object]
}

/// Seat the actual handle base and tip on the collision segment. The posed
/// sword leans across model axes, so its AABB centre is not its handle.
fn blade_transform(triangles: &[[Point3<f32>; 3]]) -> Option<Matrix4<f32>> {
    let mut low = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut high = -low;
    for p in triangles.iter().flatten() {
        for i in 0..3 {
            low[i] = low[i].min(p[i]);
            high[i] = high[i].max(p[i]);
        }
    }
    let size = high - low;
    let axis = (0..3).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap();
    if !size[axis].is_finite() || size[axis] <= 0.0 {
        return None;
    }
    let mut direction = Vector3::new(0.0, 0.0, 0.0);
    direction[axis] = 1.0;
    let base = crate::weapon_muzzle::barrel_end(triangles, -direction)?;
    let tip = crate::weapon_muzzle::barrel_end(triangles, direction)?;
    let blade = tip - base;
    Some(
        Matrix4::from(Quaternion::from_arc(
            blade.normalize(),
            Vector3::unit_y(),
            None,
        )) * Matrix4::from_scale(LENGTH / blade.magnitude())
            * Matrix4::from_translation(-Vector3::new(base.x, base.y, base.z)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Handedness, PresentationMode,
        mission::{GlobalPresentationMode, PlayerInfo},
        runtime_props::RuntimePropTransform,
    };
    use cgmath::{Deg, SquareMatrix};

    #[test]
    fn tilted_mesh_base_and_tip_match_the_damage_segment() {
        let base = Point3::new(-0.5, 0.2, 0.3);
        let tip = Point3::new(0.3, 2.0, 0.5);
        let mesh = [[
            base - vec3(0.04, 0.0, 0.0),
            base + vec3(0.04, 0.0, 0.0),
            tip,
        ]];
        let transform = blade_transform(&mesh).unwrap();
        assert!((transform.transform_point(base) - Point3::new(0.0, 0.0, 0.0)).magnitude() < 1e-6);
        assert!(
            (transform.transform_point(tip) - Point3::new(0.0, LENGTH, 0.0)).magnitude() < 1e-6
        );
        assert!(blade_transform(&[]).is_none());
        assert!(blade_transform(&[[base; 3]]).is_none());
    }

    #[test]
    fn blade_stays_in_the_ball_with_fixed_reach_in_both_hands_and_presentations() {
        for presentation in [PresentationMode::Flat, PresentationMode::Vr] {
            for hand in [Handedness::Left, Handedness::Right] {
                for roll in [-90.0, 0.0, 90.0] {
                    let mut world = World::new();
                    let controller = Matrix4::from_translation(vec3(3.0, 2.0, -1.0))
                        * Matrix4::from_angle_z(Deg(roll))
                        * Matrix4::from_angle_y(Deg(35.0));
                    let pose = if presentation == PresentationMode::Vr {
                        controller
                            * Matrix4::from(
                                crate::vr_config::get_vr_hand_model_adjustments_from_model(
                                    "amp_h", hand,
                                )
                                .rotation,
                            )
                    } else {
                        controller
                    };
                    let amp = world.add_entity(RuntimePropTransform(pose));
                    world.add_unique(GlobalPresentationMode(presentation));
                    world.add_unique(PlayerInfo {
                        pos: vec3(0.0, 0.0, 0.0),
                        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                        entity_id: EntityId::dead(),
                        inventory_entity_id: EntityId::dead(),
                        left_hand_entity_id: (hand == Handedness::Left).then_some(amp),
                        right_hand_entity_id: (hand == Handedness::Right).then_some(amp),
                    });
                    let model = if presentation == PresentationMode::Vr {
                        pose * crate::vr_config::psi_amp_fit_scale() * hand.gun_mirror()
                    } else {
                        pose
                    };
                    let center = model.transform_point(crate::psi_amp_readout::AMP_SPHERE_CENTER);
                    let (base, tip) = segment(&world, amp).unwrap();
                    assert!(
                        (base - center).magnitude() < 1e-6,
                        "blade must start inside the ball"
                    );
                    assert!(((tip - base).magnitude() - LENGTH).abs() < 1e-6);
                    assert!(
                        (tip - base)
                            .normalize()
                            .dot(controller.y.truncate().normalize())
                            > 0.9999,
                        "blade must stand out of the fist in either hand, like the other melee grips",
                    );
                    assert!((frame(&world, amp).unwrap().determinant() - 1.0).abs() < 1e-5);
                }
            }
        }
    }
}
