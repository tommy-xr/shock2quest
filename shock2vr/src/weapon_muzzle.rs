//! Muzzle geometry in the rendered weapon's local frame. Authored ID 0 wins;
//! missing points use the barrel end of the visible gun, excluding its arms.
//! Also a held gun's aim against its controller (`AimLines`), which the grip
//! tools show.
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rad, Rotation, Transform,
    Vector3, point3, vec3,
};
use collision::{Aabb, Aabb3};
use dark::{
    importers::{GLOVE_WEAPON_IMPORTER, GRIP_SURFACE_IMPORTER},
    properties::PropModelName,
    ss2_bin_obj_loader::Vhot,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{
    mission::entity_creator::CreateEntityOptions, physics::PhysicsWorld,
    runtime_props::RuntimePropVhots, scripts::Effect, vr_config::Handedness, vr_support::GripPose,
};

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MuzzleFallback {
    pub point: Point3<f32>,
    pub axis: Vector3<f32>,
}

impl MuzzleFallback {
    /// This muzzle in the frame `frame` draws its model in (a held gun's
    /// `gun_model_frame`).
    pub(crate) fn framed(self, frame: Matrix4<f32>) -> Self {
        Self {
            point: frame.transform_point(self.point),
            axis: frame.transform_vector(self.axis),
        }
    }

    /// Unit right/up/forward frame at this attachment. Item scaling affects the
    /// attachment position, never launched projectile size or velocity.
    pub(crate) fn shot_frame(self, transform: Matrix4<f32>) -> Matrix4<f32> {
        let origin = transform.transform_point(self.point);
        let forward = transform.transform_vector(self.axis).normalize();
        let up = transform.transform_vector(vec3(0.0, 1.0, 0.0)).normalize();
        let right = up.cross(forward).normalize();
        Matrix4::from_cols(
            right.extend(0.0),
            forward.cross(right).extend(0.0),
            forward.extend(0.0),
            origin.to_homogeneous(),
        )
    }
}

/// These are authored weapon frames, not an inference from the longest mesh
/// extent (which can instead measure an arm). Unknown assets retain old aim.
fn barrel_axis(name: &str) -> Option<Vector3<f32>> {
    match name.to_ascii_lowercase().trim_end_matches(".bin") {
        "atek_w" | "ar15_w" | "sg_w" | "gren_w" | "viro_w" | "al_w" => Some(vec3(0.0, 0.0, -1.0)),
        "atek_h" | "ar15_h" | "sg_h" | "lasehand" | "empgun_h" | "gren_h" | "fsn_h" | "al_h"
        | "sfg_h" | "viro_h" | "amp_h" | "laser" | "empgun" | "fsn_w" | "sfg_w" | "amp_w" => {
            Some(vec3(-1.0, 0.0, 0.0))
        }
        _ => None,
    }
}

pub(crate) fn barrel_end(
    triangles: &[[Point3<f32>; 3]],
    axis: Vector3<f32>,
) -> Option<Point3<f32>> {
    let points = triangles
        .iter()
        .flatten()
        .copied()
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
    let (near, far) = points
        .clone()
        .map(|p| p.to_vec().dot(axis))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(near, far), p| {
            (near.min(p), far.max(p))
        });
    if !far.is_finite() {
        return None;
    }
    // Bound only the front cap, not the entire gun's cross-section: a stock,
    // magazine or grip below the barrel must not pull its muzzle downward.
    let cap = points
        .filter(|p| p.to_vec().dot(axis) >= far - (far - near).max(0.01) * 0.001)
        .fold(None, |bounds: Option<Aabb3<f32>>, p| {
            Some(bounds.map_or_else(|| Aabb3::new(p, p), |b| b.grow(p)))
        })?;
    let center = cap.min + (cap.max - cap.min) * 0.5;
    Some(center + axis * (far - center.to_vec().dot(axis)))
}

/// Cached by the normal asset importers; called on model load/swap, not per shot.
pub(crate) fn load_fallback(cache: &mut AssetCache, name: &str) -> Option<MuzzleFallback> {
    let name = name.to_ascii_lowercase();
    let name = name.trim_end_matches(".bin");
    let axis = barrel_axis(name)?;
    let filename = format!("{name}.bin");
    let glove = cache.get_opt(&GLOVE_WEAPON_IMPORTER, &filename);
    let point = if let Some(source) = glove.as_ref().and_then(|s| s.as_ref().as_ref()) {
        barrel_end(&source.triangles, axis)
    } else {
        let surface = cache.get_opt(&GRIP_SURFACE_IMPORTER, &filename)?;
        barrel_end(surface.as_ref(), axis)
    }?;
    Some(MuzzleFallback { point, axis })
}

pub(crate) fn resolve(world: &World, weapon: EntityId) -> MuzzleFallback {
    let (point, axis) = muzzle(world, weapon);
    MuzzleFallback {
        point,
        axis: axis.unwrap_or_else(|| vec3(-1.0, 0.0, 0.0)),
    }
}

/// `resolve` for an item whose barrel is known (`barrel_axis`); `None` for
/// anything else, which `resolve` defaults to -X.
pub(crate) fn known(world: &World, item: EntityId) -> Option<MuzzleFallback> {
    let (point, axis) = muzzle(world, item);
    Some(MuzzleFallback { point, axis: axis? })
}

/// A weapon's muzzle point, and its barrel axis if known.
fn muzzle(world: &World, weapon: EntityId) -> (Point3<f32>, Option<Vector3<f32>>) {
    let fallback = world
        .borrow::<View<MuzzleFallback>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().copied());
    let vhots = world.borrow::<View<RuntimePropVhots>>().ok();
    let vhots = vhots
        .as_ref()
        .and_then(|v| v.get(weapon).ok())
        .map_or(&[][..], |vhots| vhots.0.as_slice());
    let axis = fallback.map(|f| f.axis).or_else(|| {
        world
            .borrow::<View<PropModelName>>()
            .ok()
            .and_then(|v| v.get(weapon).ok().and_then(|name| barrel_axis(&name.0)))
    });
    (muzzle_point(vhots, fallback.map(|f| f.point)), axis)
}

/// Where a model's muzzle is: its authored vhot 0, else the barrel-end
/// fallback, else (unprofiled assets) its lowest-ID vhot, else its origin.
fn muzzle_point(vhots: &[Vhot], fallback: Option<Point3<f32>>) -> Point3<f32> {
    let authored = vhots
        .iter()
        .min_by_key(|vhot| vhot.id)
        .map(|vhot| (vhot.id, vhot.point));
    authored
        .filter(|(id, _)| *id == 0)
        .map(|(_, point)| point)
        .or(fallback)
        .or(authored.map(|(_, point)| point))
        .unwrap_or_else(|| point3(0.0, 0.0, 0.0))
}

/// A gun's muzzle as play sets it on the model wielded in `hand`
/// (`ChangeModel`): `muzzle_point` over the glove weapon model, in the hand's
/// rendered frame. `None` for anything else, or with no known barrel.
pub fn held_muzzle(cache: &mut AssetCache, name: &str, hand: Handedness) -> Option<MuzzleFallback> {
    let name = name.to_ascii_lowercase();
    let name = name.trim_end_matches(".bin");
    if !crate::vr_weapon_grip::supports_model(name) {
        return None;
    }
    let fallback = load_fallback(cache, name)?;
    let source = cache.get_opt(&GLOVE_WEAPON_IMPORTER, &format!("{name}.bin"))?;
    let vhots = source.as_ref().as_ref()?.model.vhots();
    let muzzle = MuzzleFallback {
        point: muzzle_point(&vhots, Some(fallback.point)),
        axis: fallback.axis,
    };
    Some(muzzle.framed(crate::vr_config::gun_model_frame(name, hand)))
}

/// The controller's aim in its own frame: play's hand ray, and the way a gun
/// held with an aligned grip fires.
pub const CONTROLLER_AIM: Vector3<f32> = Vector3 {
    x: 0.0,
    y: 0.0,
    z: -1.0,
};

/// Degrees between the way an item turned by `rotation` (its grip's, in the
/// controller's frame) fires along `barrel` (its muzzle axis) and the
/// controller's aim. 0 fires straight; roll about the barrel does not count.
pub fn aim_error_deg(rotation: Quaternion<f32>, barrel: Vector3<f32>) -> f32 {
    let firing = rotation.rotate_vector(barrel);
    // atan2 stays exact near 0°, where acos of a rounded cosine does not.
    let (sin, cos) = (
        firing.cross(CONTROLLER_AIM).magnitude(),
        firing.dot(CONTROLLER_AIM),
    );
    Deg::from(Rad(sin.atan2(cos))).0
}

/// A held gun's aim: its muzzle and the way it fires, and the controller's
/// position and the way it aims, all in the space `model` and `controller`
/// are in; and how far apart the two directions are (`aim_error_deg`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimLines {
    pub muzzle: Vector3<f32>,
    pub firing: Vector3<f32>,
    pub controller: Vector3<f32>,
    pub aim: Vector3<f32>,
    pub error_deg: f32,
}

impl AimLines {
    /// For the item's model drawn at `model` and `scale`, held by
    /// `controller`.
    pub fn new(model: GripPose, scale: f32, muzzle: MuzzleFallback, controller: GripPose) -> Self {
        Self {
            muzzle: model.point(muzzle.point.to_vec() * scale),
            firing: model.rotation.rotate_vector(muzzle.axis).normalize(),
            controller: controller.position,
            aim: controller.rotation.rotate_vector(CONTROLLER_AIM),
            error_deg: aim_error_deg(controller.rotation.invert() * model.rotation, muzzle.axis),
        }
    }
}

/// Keep a forward spawn offset on the firing side of any intervening surface.
/// The caller filters held items; the player's own capsule is always excluded.
pub(crate) fn clamp_projectile_spawn(
    physics: &PhysicsWorld,
    origin: Point3<f32>,
    requested: Point3<f32>,
    radius: f32,
    point_vs_terrain: bool,
    can_hit: &dyn Fn(EntityId) -> bool,
) -> Point3<f32> {
    let delta = requested - origin;
    let distance = delta.magnitude();
    if distance <= 1.0e-6 || !distance.is_finite() {
        return origin;
    }
    let direction = delta / distance;
    // A tiny sphere also covers point projectiles without dropping anonymous
    // level colliders from the query's entity predicate.
    origin
        + direction
            * physics.projectile_spawn_distance(
                origin,
                direction,
                distance,
                radius.max(0.0001),
                point_vs_terrain,
                can_hit,
            )
}

/// A gun flash entity at `position` in `root_transform`'s space, its mesh
/// turned onto `axis` (the shot direction in that space). Flash meshes are
/// authored along -X, like held gun barrels.
pub(crate) fn flash_entity(
    template_id: i32,
    root_transform: Matrix4<f32>,
    position: Point3<f32>,
    axis: Vector3<f32>,
    options: CreateEntityOptions,
) -> Effect {
    Effect::CreateEntity {
        template_id,
        position,
        orientation: Quaternion::from_arc(vec3(-1.0, 0.0, 0.0), axis, None),
        root_transform,
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::CollisionGroup;
    use cgmath::Quaternion;

    #[test]
    fn fallback_uses_front_cap_not_stock_or_arm_extents() {
        let triangles = [
            [
                point3(-2.0, 3.0, -1.0),
                point3(-2.0, 3.0, 1.0),
                point3(-2.0, 5.0, 0.0),
            ],
            [
                point3(0.0, -10.0, -5.0),
                point3(0.0, -10.0, 5.0),
                point3(0.0, 0.0, 0.0),
            ],
        ];
        assert_eq!(
            barrel_end(&triangles, vec3(-1.0, 0.0, 0.0)),
            Some(point3(-2.0, 4.0, 0.0))
        );
        assert_eq!(barrel_end(&[], vec3(-1.0, 0.0, 0.0)), None);
    }

    #[test]
    fn authored_muzzle_id_wins_over_geometry_and_other_attachments() {
        let mut world = World::new();
        let point = point3(-1.0, 0.2, 0.1);
        let weapon = world.add_entity((
            MuzzleFallback {
                point: point3(-2.0, 0.0, 0.0),
                axis: vec3(-1.0, 0.0, 0.0),
            },
            RuntimePropVhots(vec![
                dark::ss2_bin_obj_loader::Vhot {
                    id: 1,
                    point: point3(-3.0, 0.0, 0.0),
                },
                dark::ss2_bin_obj_loader::Vhot { id: 0, point },
            ]),
        ));
        assert_eq!(resolve(&world, weapon).point, point);
    }

    #[test]
    fn aim_error_reads_how_far_a_grip_turns_the_barrel_off_the_controller() {
        use cgmath::{Deg, Rotation3};
        let barrel = vec3(-1.0, 0.0, 0.0);
        // Turns the -X barrel onto the controller's -Z.
        let straight = Quaternion::from_angle_y(Deg(-90.0));
        assert!(aim_error_deg(straight, barrel).abs() < 1e-4);
        let rolled = straight * Quaternion::from_angle_x(Deg(40.0));
        assert!(
            aim_error_deg(rolled, barrel).abs() < 1e-4,
            "roll is not aim"
        );
        for (offset, expected) in [
            (Quaternion::from_angle_y(Deg(12.0)), 12.0),
            (Quaternion::from_angle_x(Deg(-7.0)), 7.0),
            (Quaternion::from_angle_y(Deg(180.0)), 180.0),
        ] {
            let error = aim_error_deg(offset * straight, barrel);
            assert!((error - expected).abs() < 1e-3, "{error} vs {expected}");
        }
        // The lines read the grip's error wherever the controller is.
        let controller = GripPose {
            position: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::from_angle_z(Deg(30.0)) * Quaternion::from_angle_x(Deg(50.0)),
        };
        let grip = Quaternion::from_angle_y(Deg(12.0)) * straight;
        let lines = AimLines::new(
            GripPose {
                position: controller.position,
                rotation: controller.rotation * grip,
            },
            1.0,
            MuzzleFallback {
                point: point3(-1.0, 0.0, 0.0),
                axis: barrel,
            },
            controller,
        );
        assert!((lines.error_deg - 12.0).abs() < 1e-3);
        assert!((Deg::from(lines.firing.angle(lines.aim)).0 - 12.0).abs() < 1e-3);
    }

    #[test]
    fn terrain_point_clearance_does_not_shrink_object_clearance() {
        for terrain in [false, true] {
            let mut physics = PhysicsWorld::new();
            physics.add_kinematic(
                EntityId::from_inner(20).unwrap(),
                vec3(0.3, 0.0, 1.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 0.0),
                vec3(0.2, 4.0, 0.02),
                if terrain {
                    CollisionGroup::world_for_test()
                } else {
                    CollisionGroup::entity()
                },
                false,
            );
            let mut player =
                physics.create_player(vec3(10.0, 0.0, 0.0), EntityId::from_inner(21).unwrap());
            physics.update(vec3(0.0, 0.0, 0.0), &mut player);
            let start = point3(0.0, 0.0, 0.0);
            let muzzle = point3(0.0, 0.0, 2.0);
            let clamped = clamp_projectile_spawn(&physics, start, muzzle, 0.4, true, &|_| true);
            assert_eq!(clamped == muzzle, terrain);
            assert!(clamp_projectile_spawn(&physics, start, muzzle, 0.4, false, &|_| true).z < 1.0);
        }
    }

    #[test]
    fn forward_spawn_offset_cannot_cross_a_thin_obstacle() {
        let mut physics = PhysicsWorld::new();
        let wall = EntityId::from_inner(20).unwrap();
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 1.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(4.0, 4.0, 0.02),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            physics.create_player(vec3(10.0, 0.0, 0.0), EntityId::from_inner(21).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let start = point3(0.0, 0.0, 0.0);
        let muzzle = point3(0.0, 0.0, 2.0);
        let clamped = clamp_projectile_spawn(&physics, start, muzzle, 0.0, false, &|_| true);
        assert!(
            clamped.z > 0.9 && clamped.z < 0.99,
            "must stay before wall: {clamped:?}"
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, point3(0.0, 0.0, 0.9), start, 0.2, false, &|_| {
                true
            }),
            start,
            "a shot directed away from an initially overlapping wall must clear it",
        );
        let grenade = clamp_projectile_spawn(&physics, start, muzzle, 0.2, false, &|_| true);
        assert!(
            grenade.z + 0.2 < 0.99,
            "grenade must fit before the wall: {grenade:?}"
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, start, muzzle, 0.0, false, &|id| id != wall),
            muzzle
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, start, start, 0.0, false, &|_| true),
            start
        );
    }
}
