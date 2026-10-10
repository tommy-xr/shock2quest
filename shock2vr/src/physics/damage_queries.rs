//! Research backend: immutable damage shapes, grouped independently of actor capsules.
//! Snapshot bounds cover this pose only, not motion between two poses.
use rapier3d::{
    parry::{bounding_volume::Aabb, query::RayCast},
    prelude::*,
};

struct DamageShape {
    handle: ColliderHandle,
    shape: SharedShape,
    pose: Isometry<Real>,
}

struct DamageGroup {
    // None disables rejection: an uncertain bound must not hide its children.
    bounds: Option<Aabb>,
    shapes: Vec<DamageShape>,
}

impl DamageGroup {
    fn new(shapes: Vec<DamageShape>) -> Self {
        let mut bounds: Option<Aabb> = None;
        let mut valid = true;
        for child in &shapes {
            let child_bounds = child.shape.compute_aabb(&child.pose);
            if !child_bounds
                .mins
                .iter()
                .chain(child_bounds.maxs.iter())
                .all(|v| v.is_finite())
            {
                valid = false;
                break;
            }
            bounds = Some(match bounds {
                None => child_bounds,
                Some(acc) => Aabb::new(
                    acc.mins.inf(&child_bounds.mins),
                    acc.maxs.sup(&child_bounds.maxs),
                ),
            });
        }
        let bounds = if valid {
            bounds.and_then(|bounds| {
                // Outward slack protects grazing hits against roundoff in transformed bounds.
                let scale = bounds
                    .mins
                    .coords
                    .abs()
                    .max()
                    .max(bounds.maxs.coords.abs().max());
                let margin = (8.0 * Real::EPSILON * (1.0 + scale)).max(1.0e-5);
                let padded = Aabb::new(
                    bounds.mins - Vector::repeat(margin),
                    bounds.maxs + Vector::repeat(margin),
                );
                padded
                    .mins
                    .iter()
                    .chain(padded.maxs.iter())
                    .all(|v| v.is_finite())
                    .then_some(padded)
            })
        } else {
            None
        };
        Self { bounds, shapes }
    }
}

struct DamageSnapshot {
    groups: Vec<DamageGroup>,
}

impl DamageSnapshot {
    /// `max_toi` also lets a caller clip against its nearest world hit. Direction
    /// follows Parry's ray convention: unit direction means distances in world units.
    fn cast_ray(
        &self,
        ray: &Ray,
        max_toi: Real,
        shape_tests: &mut usize,
    ) -> Option<(ColliderHandle, RayIntersection)> {
        if !ray
            .origin
            .iter()
            .chain(ray.dir.iter())
            .all(|v| v.is_finite())
            || !ray.dir.norm_squared().is_finite()
            || ray.dir.norm_squared() == 0.0
            || max_toi.is_nan()
            || max_toi < 0.0
        {
            return None;
        }
        let mut result = None;
        let mut nearest = max_toi;
        for group in &self.groups {
            if group
                .bounds
                .as_ref()
                .is_some_and(|bounds| bounds.cast_local_ray(ray, nearest, true).is_none())
            {
                continue;
            }
            for child in &group.shapes {
                *shape_tests += 1;
                if let Some(hit) =
                    child
                        .shape
                        .cast_ray_and_get_normal(&child.pose, ray, nearest, true)
                {
                    nearest = hit.time_of_impact;
                    result = Some((child.handle, hit));
                }
            }
        }
        result
    }
}

#[cfg(feature = "damage-query-audit")]
impl super::PhysicsWorld {
    pub(crate) fn register_damage_hitbox(
        &mut self,
        child: shipyard::EntityId,
        owner: shipyard::EntityId,
    ) {
        self.damage_owners.insert(child, owner);
    }

    /// Run after Rapier updates its BVH. Gameplay continues to use Rapier's result.
    pub(super) fn audit_damage_queries(&mut self) {
        self.damage_audit_steps += 1;
        if self.damage_audit_steps % 600 != 0 {
            return;
        }
        let build_started = std::time::Instant::now();
        let mut grouped = std::collections::HashMap::<u64, Vec<DamageShape>>::new();
        for (handle, collider) in self.collider_set.iter() {
            if !collider.is_enabled()
                || collider.is_sensor()
                || !collider
                    .collision_groups()
                    .memberships
                    .intersects(super::InternalCollisionGroups::HITBOX.bits.into())
            {
                continue;
            }
            let Some(child) = shipyard::EntityId::from_inner(collider.user_data as u64) else {
                continue;
            };
            // Unknown owners get their own group, never a guessed actor capsule.
            let owner = self.damage_owners.get(&child).copied().unwrap_or(child);
            grouped.entry(owner.inner()).or_default().push(DamageShape {
                handle,
                shape: collider.shared_shape().clone(),
                pose: *collider.position(),
            });
        }
        let snapshot = DamageSnapshot {
            groups: grouped.into_values().map(DamageGroup::new).collect(),
        };
        let build_us = build_started.elapsed().as_secs_f64() * 1e6;
        let mut rays = Vec::new();
        let mut shapes = 0;
        let mut skipped_probes = 0;
        for group in &snapshot.groups {
            for child in &group.shapes {
                shapes += 1;
                let bounds = child.shape.compute_aabb(&child.pose);
                let center = bounds.center();
                if !center.iter().all(|v| v.is_finite()) || !bounds.extents().norm().is_finite() {
                    skipped_probes += 1;
                    continue;
                }
                // Probe every live limb, including ones outside the actor capsule.
                for axis in [Vector::x(), Vector::y(), Vector::z()] {
                    rays.push(Ray::new(
                        center - axis * (bounds.extents().norm() + 1.0),
                        axis,
                    ));
                }
                rays.push(Ray::new(center, Vector::x()));
            }
        }
        let mut tests = 0;
        let started = std::time::Instant::now();
        let candidates: Vec<_> = rays
            .iter()
            .map(|ray| snapshot.cast_ray(ray, 1000.0, &mut tests))
            .collect();
        let query_us = started.elapsed().as_secs_f64() * 1e6;
        let filter = QueryFilter::new()
            .exclude_sensors()
            .groups(InteractionGroups::new(
                super::InternalCollisionGroups::RAYCAST.bits.into(),
                super::InternalCollisionGroups::HITBOX.bits.into(),
                Default::default(),
            ));
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        let started = std::time::Instant::now();
        let mut mismatches = 0;
        let mut ties = 0;
        for (ray, candidate) in rays.iter().zip(candidates) {
            let reference = queries.cast_ray_and_get_normal(ray, 1000.0, true);
            match (candidate, reference) {
                (None, None) => {}
                (Some((a, ah)), Some((b, bh)))
                    if (ah.time_of_impact - bh.time_of_impact).abs()
                        <= 1e-4 * (1.0 + bh.time_of_impact.abs()) =>
                {
                    if a != b {
                        ties += 1;
                    }
                }
                _ => mismatches += 1,
            }
        }
        eprintln!(
            "SHOCK2QUEST_DAMAGE_QUERY_AUDIT {}",
            serde_json::json!({
                "step": self.damage_audit_steps, "groups": snapshot.groups.len(), "shapes": shapes,
                "rays": rays.len(), "skipped_probes": skipped_probes, "shape_tests": tests, "ungated_shape_tests": rays.len() * shapes,
                "distance_mismatches": mismatches, "equal_distance_different_proxy": ties,
                "bounds_build_us": build_us, "grouped_query_us": query_us,
                "rapier_query_us": started.elapsed().as_secs_f64() * 1e6,
            })
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    fn shape(id: u32, geometry: SharedShape, pose: Isometry<Real>) -> DamageShape {
        DamageShape {
            handle: ColliderHandle::from_raw_parts(id, 0),
            shape: geometry,
            pose,
        }
    }

    fn brute(snapshot: &DamageSnapshot, ray: &Ray, limit: Real) -> Option<Real> {
        snapshot
            .groups
            .iter()
            .flat_map(|g| &g.shapes)
            .filter_map(|child| child.shape.cast_ray(&child.pose, ray, limit, true))
            .min_by(Real::total_cmp)
    }

    fn assert_parity(snapshot: &DamageSnapshot, ray: &Ray, limit: Real) {
        let expected = brute(snapshot, ray, limit);
        let actual = snapshot
            .cast_ray(ray, limit, &mut 0)
            .map(|(_, hit)| hit.time_of_impact);
        match (actual, expected) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 1e-4, "{a} != {b}: {ray:?}"),
            (a, b) => assert_eq!(a, b, "{ray:?}"),
        }
    }

    #[test]
    fn extended_limb_is_hit_even_when_ray_misses_physical_body_bounds() {
        let body = Aabb::new(point![-0.5, -1.0, -0.5], point![0.5, 1.0, 0.5]);
        let ray = Ray::new(point![3.0, 0.0, -5.0], Vector::z());
        assert!(body.cast_local_ray(&ray, 10.0, true).is_none());
        let snapshot = DamageSnapshot {
            groups: vec![DamageGroup::new(vec![shape(
                1,
                SharedShape::cuboid(2.0, 0.05, 0.05),
                Isometry::translation(1.5, 0.0, 0.0),
            )])],
        };
        assert!(snapshot.cast_ray(&ray, 10.0, &mut 0).is_some());
        assert_parity(&snapshot, &ray, 10.0);
    }

    #[test]
    fn long_skinny_swing_is_contained_for_every_sampled_pose() {
        // Synthetic pipe-length volume: this exercises the geometry invariant,
        // not a claim that the actual hybrid clip has been replayed.
        for degrees in (-180..=180).step_by(3) {
            let rotation = Rotation::from_euler_angles(0.31, (degrees as f32).to_radians(), 0.67);
            let root = Isometry::from_parts(Translation::new(-27.0, 3.0, 41.0), rotation);
            let pose = root * Isometry::translation(2.0, 0.4, 0.0);
            let child = shape(1, SharedShape::cuboid(2.0, 0.015, 0.015), pose);
            let child_bounds = child.shape.compute_aabb(&pose);
            let snapshot = DamageSnapshot {
                groups: vec![DamageGroup::new(vec![child])],
            };
            let outer = snapshot.groups[0].bounds.unwrap();
            for axis in 0..3 {
                assert!(outer.mins[axis] <= child_bounds.mins[axis]);
                assert!(outer.maxs[axis] >= child_bounds.maxs[axis]);
            }
            for x in [-1.999, 0.0, 1.999] {
                let ray = Ray::new(pose * point![x, 0.0, -3.0], pose.rotation * Vector::z());
                assert!(snapshot.cast_ray(&ray, 10.0, &mut 0).is_some());
                assert_parity(&snapshot, &ray, 10.0);
            }
        }
    }

    #[test]
    fn nearest_hit_is_independent_of_group_order_and_clipped_by_world_hit() {
        let make = |id, x| {
            DamageGroup::new(vec![shape(
                id,
                SharedShape::ball(0.5),
                Isometry::translation(x, 0.0, 0.0),
            )])
        };
        let mut snapshot = DamageSnapshot {
            groups: vec![make(2, 8.0), make(1, 3.0)],
        };
        let ray = Ray::new(Point::origin(), Vector::x());
        for _ in 0..2 {
            assert_eq!(
                snapshot.cast_ray(&ray, 20.0, &mut 0).unwrap().0,
                ColliderHandle::from_raw_parts(1, 0)
            );
            assert!(snapshot.cast_ray(&ray, 2.0, &mut 0).is_none());
            snapshot.groups.reverse();
        }
    }

    #[test]
    fn bounds_misses_do_no_detailed_tests_and_false_positives_do_not_become_hits() {
        let snapshot = DamageSnapshot {
            groups: vec![DamageGroup::new(vec![shape(
                1,
                SharedShape::ball(1.0),
                Isometry::identity(),
            )])],
        };
        let mut tests = 0;
        assert!(
            snapshot
                .cast_ray(
                    &Ray::new(point![2.0, 2.0, -5.0], Vector::z()),
                    10.0,
                    &mut tests
                )
                .is_none()
        );
        assert_eq!(tests, 0);
        assert!(
            snapshot
                .cast_ray(
                    &Ray::new(point![0.99, 0.99, -5.0], Vector::z()),
                    10.0,
                    &mut tests
                )
                .is_none()
        );
        assert_eq!(tests, 1);
    }

    #[test]
    fn grazing_and_inside_rays_and_unknown_bounds_keep_hits() {
        let child = shape(1, SharedShape::cuboid(1.0, 1.0, 1.0), Isometry::identity());
        let mut snapshot = DamageSnapshot {
            groups: vec![DamageGroup::new(vec![child])],
        };
        for bounds_known in [true, false] {
            if !bounds_known {
                snapshot.groups[0].bounds = None;
            }
            for ray in [
                Ray::new(point![1.0, 0.0, -3.0], Vector::z()),
                Ray::new(Point::origin(), Vector::x()),
            ] {
                assert!(snapshot.cast_ray(&ray, 10.0, &mut 0).is_some());
                assert_parity(&snapshot, &ray, 10.0);
            }
        }
    }

    #[test]
    fn grouped_queries_match_brute_shapes_for_seeded_oblique_rays() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0xDABA_2026);
        let mut groups = Vec::new();
        for group in 0..12 {
            let mut shapes = Vec::new();
            for joint in 0..8 {
                let pose = Isometry::new(
                    vector![
                        rng.gen_range(-8.0..8.0),
                        rng.gen_range(-4.0..4.0),
                        rng.gen_range(-8.0..8.0)
                    ],
                    vector![
                        rng.gen_range(-3.0..3.0),
                        rng.gen_range(-3.0..3.0),
                        rng.gen_range(-3.0..3.0)
                    ],
                );
                let geometry = if joint % 2 == 0 {
                    SharedShape::capsule_y(1.5, 0.03)
                } else {
                    SharedShape::cuboid(0.03, 0.1, 2.0)
                };
                shapes.push(shape(group * 8 + joint, geometry, pose));
            }
            groups.push(DamageGroup::new(shapes));
        }
        let snapshot = DamageSnapshot { groups };
        for _ in 0..4000 {
            let origin = point![
                rng.gen_range(-10.0..10.0),
                rng.gen_range(-6.0..6.0),
                rng.gen_range(-10.0..10.0)
            ];
            let direction = vector![
                rng.gen_range(-1.0..1.0),
                rng.gen_range(-1.0..1.0),
                rng.gen_range(-1.0..1.0)
            ]
            .normalize();
            assert_parity(
                &snapshot,
                &Ray::new(origin, direction),
                rng.gen_range(0.0..30.0),
            );
        }
    }

    #[test]
    fn overflowing_bound_disables_rejection() {
        let group = DamageGroup::new(vec![shape(
            1,
            SharedShape::ball(1.0),
            Isometry::translation(Real::MAX, 0.0, 0.0),
        )]);
        assert!(group.bounds.is_none());
        assert_eq!(group.shapes.len(), 1);
    }

    #[test]
    fn empty_and_degenerate_queries_are_safe() {
        let snapshot = DamageSnapshot {
            groups: vec![DamageGroup::new(Vec::new())],
        };
        assert!(
            snapshot
                .cast_ray(&Ray::new(Point::origin(), Vector::x()), 10.0, &mut 0)
                .is_none()
        );
        assert!(
            snapshot
                .cast_ray(&Ray::new(Point::origin(), Vector::zeros()), 10.0, &mut 0)
                .is_none()
        );
        assert!(
            snapshot
                .cast_ray(
                    &Ray::new(point![Real::NAN, 0.0, 0.0], Vector::x()),
                    10.0,
                    &mut 0
                )
                .is_none()
        );
    }
}
