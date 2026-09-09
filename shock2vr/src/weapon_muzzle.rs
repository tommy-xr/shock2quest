//! Muzzle geometry in the rendered weapon's local frame. Authored ID 0 wins;
//! missing points use the barrel end of the visible gun, excluding its arms.
use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, point3, vec3};
use collision::{Aabb, Aabb3};
use dark::{
    importers::{GLOVE_WEAPON_IMPORTER, GRIP_SURFACE_IMPORTER},
    properties::PropModelName,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, runtime_props::RuntimePropVhots};

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct MuzzleFallback {
    pub point: Point3<f32>,
    pub axis: Vector3<f32>,
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

fn barrel_end(triangles: &[[Point3<f32>; 3]], axis: Vector3<f32>) -> Option<Point3<f32>> {
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
    let fallback = world
        .borrow::<View<MuzzleFallback>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().copied());
    let authored = world.borrow::<View<RuntimePropVhots>>().ok().and_then(|v| {
        v.get(weapon).ok().and_then(|vhots| {
            vhots
                .0
                .iter()
                .min_by_key(|vhot| vhot.id)
                .map(|vhot| (vhot.id, vhot.point))
        })
    });
    let axis = fallback
        .map(|f| f.axis)
        .or_else(|| {
            world
                .borrow::<View<PropModelName>>()
                .ok()
                .and_then(|v| v.get(weapon).ok().and_then(|name| barrel_axis(&name.0)))
        })
        .unwrap_or_else(|| vec3(-1.0, 0.0, 0.0));
    MuzzleFallback {
        point: authored
            .filter(|(id, _)| *id == 0)
            .map(|(_, point)| point)
            .or(fallback.map(|f| f.point))
            // Unprofiled assets retain their previous lowest-ID fallback.
            .or(authored.map(|(_, point)| point))
            .unwrap_or_else(|| point3(0.0, 0.0, 0.0)),
        axis,
    }
}

/// Keep a forward spawn offset on the firing side of any intervening surface.
/// The caller filters held items; the player's own capsule is always excluded.
pub(crate) fn clamp_projectile_spawn(
    physics: &PhysicsWorld,
    origin: Point3<f32>,
    requested: Point3<f32>,
    radius: f32,
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
                can_hit,
            )
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
        let clamped = clamp_projectile_spawn(&physics, start, muzzle, 0.0, &|_| true);
        assert!(
            clamped.z > 0.9 && clamped.z < 0.99,
            "must stay before wall: {clamped:?}"
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, point3(0.0, 0.0, 0.9), start, 0.2, &|_| true),
            start,
            "a shot directed away from an initially overlapping wall must clear it",
        );
        let grenade = clamp_projectile_spawn(&physics, start, muzzle, 0.2, &|_| true);
        assert!(
            grenade.z + 0.2 < 0.99,
            "grenade must fit before the wall: {grenade:?}"
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, start, muzzle, 0.0, &|id| id != wall),
            muzzle
        );
        assert_eq!(
            clamp_projectile_spawn(&physics, start, start, 0.0, &|_| true),
            start
        );
    }
}
