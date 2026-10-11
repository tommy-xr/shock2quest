//! Query-only damage geometry. ECS limb entities still own damage forwarding;
//! neither these shapes nor their enclosing bounds participate in the solver.
use std::collections::{BTreeMap, HashSet};

use rapier3d::{
    parry::{
        bounding_volume::{Aabb, BoundingVolume},
        query::{self, RayCast, ShapeCastOptions},
    },
    prelude::*,
};
use shipyard::EntityId;

use super::util::{npoint_to_cgvec, nvec_to_cgmath};
use super::{CollisionContact, CollisionEvent, PhysicsWorld, SweepStop};

struct Limb {
    owner: EntityId,
    joint: u32,
    shape: SharedShape,
    pose: Isometry<Real>,
    pending: Isometry<Real>,
    active: bool,
    seen: bool,
}

#[derive(Default)]
struct Group {
    bounds: Option<Aabb>,
    limbs: Vec<EntityId>,
}

#[derive(Default)]
pub(super) struct QueryHitboxes {
    limbs: BTreeMap<EntityId, Limb>,
    groups: BTreeMap<EntityId, Group>,
    contacts: HashSet<(EntityId, EntityId)>,
}

impl QueryHitboxes {
    pub fn begin_frame(&mut self) {
        for limb in self.limbs.values_mut() {
            limb.seen = false;
        }
    }

    pub fn stage(
        &mut self,
        owner: EntityId,
        entity: EntityId,
        joint: u32,
        pose: Isometry<Real>,
        shape: impl FnOnce() -> SharedShape,
    ) {
        let limb = self.limbs.entry(entity).or_insert_with(|| Limb {
            owner,
            joint,
            shape: shape(),
            pose,
            pending: pose,
            active: false,
            seen: true,
        });
        limb.owner = owner;
        limb.joint = joint;
        limb.pending = pose;
        limb.seen = true;
    }

    pub fn finish_frame(&mut self) {
        let previous = self.limbs.len();
        self.limbs.retain(|_, limb| limb.seen);
        if previous != self.limbs.len() {
            self.rebuild_groups();
        }
    }

    // Mirror position-based Rapier kinematics: next pose becomes queryable
    // after the physics step, not when animation queues its next target.
    pub fn commit(&mut self) {
        for limb in self.limbs.values_mut() {
            limb.pose = limb.pending;
            limb.active = true;
        }
        self.rebuild_groups();
    }

    pub fn remove(&mut self, entity: EntityId) {
        self.limbs
            .retain(|id, limb| *id != entity && limb.owner != entity);
        self.contacts
            .retain(|(weapon, limb)| *weapon != entity && self.limbs.contains_key(limb));
        self.rebuild_groups();
    }

    fn rebuild_groups(&mut self) {
        self.groups.clear();
        for (&id, limb) in &self.limbs {
            if limb.active {
                self.groups.entry(limb.owner).or_default().limbs.push(id);
            }
        }
        for group in self.groups.values_mut() {
            group.limbs.sort_by_key(|id| (self.limbs[id].joint, *id));
            group.bounds =
                super::damage_queries::conservative_bounds(group.limbs.iter().map(|id| {
                    let limb = &self.limbs[id];
                    limb.shape.compute_aabb(&limb.pose)
                }));
        }
    }

    pub fn geometry(&self, entity: EntityId) -> Option<(&dyn Shape, &Isometry<Real>)> {
        let limb = self.limbs.get(&entity).filter(|limb| limb.active)?;
        Some((limb.shape.as_ref(), &limb.pose))
    }

    pub fn bounds(&self, entity: EntityId) -> Option<Aabb> {
        let limb = self.limbs.get(&entity).filter(|limb| limb.active)?;
        Some(limb.shape.compute_aabb(&limb.pose))
    }

    /// Placement/recovery queries must retain limb occupancy even though these
    /// volumes no longer live in Rapier. Unsupported pairs conservatively block.
    pub fn intersects(
        &self,
        pose: &Isometry<Real>,
        shape: &dyn Shape,
        allowed: &dyn Fn(EntityId) -> bool,
    ) -> bool {
        let bounds = shape.compute_aabb(pose);
        self.groups.values().any(|group| {
            !group.bounds.is_some_and(|b| !b.intersects(&bounds))
                && group.limbs.iter().any(|id| {
                    let limb = &self.limbs[id];
                    allowed(*id)
                        && query::intersection_test(pose, shape, &limb.pose, limb.shape.as_ref())
                            .unwrap_or(true)
                })
        })
    }

    pub fn cast_ray(
        &self,
        ray: &Ray,
        limit: Real,
        allowed: &dyn Fn(EntityId) -> bool,
    ) -> Option<(EntityId, RayIntersection)> {
        let mut result: Option<(EntityId, RayIntersection)> = None;
        let mut nearest = limit;
        for group in self.groups.values() {
            if group
                .bounds
                .is_some_and(|bounds| bounds.cast_local_ray(ray, nearest, true).is_none())
            {
                continue;
            }
            for id in &group.limbs {
                if !allowed(*id) {
                    continue;
                }
                let limb = &self.limbs[id];
                if let Some(hit) = limb
                    .shape
                    .cast_ray_and_get_normal(&limb.pose, ray, nearest, true)
                {
                    // Exact ties prefer the lowest joint index within an owner,
                    // independent of proxy allocation or group insertion order.
                    if result.as_ref().is_none_or(|(previous, old)| {
                        hit.time_of_impact < old.time_of_impact
                            || (hit.time_of_impact == old.time_of_impact
                                && (limb.owner, limb.joint, *id)
                                    < (
                                        self.limbs[previous].owner,
                                        self.limbs[previous].joint,
                                        *previous,
                                    ))
                    }) {
                        nearest = hit.time_of_impact;
                        result = Some((*id, hit));
                    }
                }
            }
        }
        result
    }

    /// Translation sweep, matching the existing held-item drive. The bound
    /// covers the whole translated path, including the collider's local offset.
    pub fn sweep(
        &self,
        pose: &Isometry<Real>,
        velocity: &Vector<Real>,
        shape: &dyn Shape,
        limit: Real,
        skin: Real,
    ) -> Option<(Real, SweepStop)> {
        let mut end = *pose;
        end.translation.vector += velocity * limit;
        let swept = shape
            .compute_aabb(pose)
            .merged(&shape.compute_aabb(&end))
            .loosened(skin);
        let mut result: Option<(Real, SweepStop)> = None;
        let mut nearest = limit;
        for group in self.groups.values() {
            if group
                .bounds
                .is_some_and(|bounds| !bounds.intersects(&swept))
            {
                continue;
            }
            for id in &group.limbs {
                let limb = &self.limbs[id];
                let options = ShapeCastOptions {
                    max_time_of_impact: nearest,
                    target_distance: skin,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                };
                if let Ok(Some(hit)) = query::cast_shapes(
                    &limb.pose,
                    &Vector::zeros(),
                    limb.shape.as_ref(),
                    pose,
                    velocity,
                    shape,
                    options,
                ) {
                    if hit.time_of_impact >= nearest {
                        continue;
                    }
                    nearest = hit.time_of_impact;
                    result = Some((
                        nearest,
                        SweepStop {
                            limb: *id,
                            point: npoint_to_cgvec(limb.pose * hit.witness1),
                            normal: -nvec_to_cgmath(limb.pose.rotation * hit.normal1.into_inner()),
                            surface_material: None,
                        },
                    ));
                }
            }
        }
        result
    }
}

impl PhysicsWorld {
    pub(crate) fn query_hitbox_position(&self, entity: EntityId) -> Option<[f32; 3]> {
        let (_, pose) = self.query_hitboxes.geometry(entity)?;
        Some(pose.translation.vector.into())
    }

    pub(crate) fn begin_query_hitbox_frame(&mut self) {
        self.query_hitboxes.begin_frame();
    }
    pub(crate) fn finish_query_hitbox_frame(&mut self) {
        self.query_hitboxes.finish_frame();
    }
    pub(crate) fn stage_query_hitbox(
        &mut self,
        owner: EntityId,
        entity: EntityId,
        joint: u32,
        position: cgmath::Vector3<f32>,
        rotation: cgmath::Quaternion<f32>,
        shape: impl FnOnce() -> SharedShape,
    ) {
        let pose = Isometry::from_parts(
            Translation::from(super::util::vec_to_nvec(position)),
            super::util::quat_to_nquat(rotation),
        );
        self.query_hitboxes.stage(owner, entity, joint, pose, shape);
    }

    /// Replacement for narrow-phase limb contact starts. Sweep events still
    /// handle fast translation; this catches resting/rotating weapons and moving
    /// creatures at the committed pose. The script retains per-owner cooldowns.
    pub(super) fn query_hitbox_contacts(&mut self) -> Vec<CollisionEvent> {
        let mut contacts = BTreeMap::new();
        let prediction = self.integration_parameters.prediction_distance();
        for weapon in self.held_item_drives.keys() {
            let Some(body) = self.rigid_body_set.get(*weapon).filter(|b| b.is_enabled()) else {
                continue;
            };
            let Some(weapon_entity) = EntityId::from_inner(body.user_data as u64) else {
                continue;
            };
            for handle in body.colliders() {
                let collider = &self.collider_set[*handle];
                if !collider.is_enabled()
                    || collider.is_sensor()
                    || !collider
                        .collision_groups()
                        .test(super::CollisionGroup::hitbox().collision)
                {
                    continue;
                }
                let bounds = collider.compute_aabb().loosened(prediction);
                for group in self.query_hitboxes.groups.values() {
                    if group.bounds.is_some_and(|b| !b.intersects(&bounds)) {
                        continue;
                    }
                    for id in &group.limbs {
                        let limb = &self.query_hitboxes.limbs[id];
                        let a = weapon_entity.inner() as u128;
                        // An exemption against the limb's creature (a shot and
                        // its launcher) covers all of that creature's limbs.
                        let ignored = |other: EntityId| {
                            let b = other.inner() as u128;
                            self.ignored_collision_pairs.contains(&(a.min(b), a.max(b)))
                        };
                        if limb.owner == weapon_entity || ignored(*id) || ignored(limb.owner) {
                            continue;
                        }
                        if let Ok(Some(contact)) = query::contact(
                            collider.position(),
                            collider.shape(),
                            &limb.pose,
                            limb.shape.as_ref(),
                            prediction,
                        ) {
                            let key = (weapon_entity, *id);
                            let entry = contacts.entry(key).or_insert(contact);
                            if contact.dist < entry.dist {
                                *entry = contact;
                            }
                        }
                    }
                }
            }
        }
        let mut events: Vec<_> = contacts
            .iter()
            .filter(|(key, _)| !self.query_hitboxes.contacts.contains(key))
            .map(
                |(&(weapon, limb), contact)| CollisionEvent::CollisionStarted {
                    entity1_id: weapon,
                    entity2_id: limb,
                    contact: Some(CollisionContact {
                        point: npoint_to_cgvec(Point::from(
                            (contact.point1.coords + contact.point2.coords) * 0.5,
                        )),
                        normal: nvec_to_cgmath(contact.normal1.into_inner()),
                        closing_speed: None,
                        surface_material: None,
                    }),
                },
            )
            .collect();
        events.sort_by_key(|event| match event {
            CollisionEvent::CollisionStarted {
                entity1_id,
                entity2_id,
                ..
            } => {
                let limb = &self.query_hitboxes.limbs[entity2_id];
                (*entity1_id, limb.owner, limb.joint, *entity2_id)
            }
            _ => unreachable!(),
        });
        self.query_hitboxes.contacts = contacts.into_keys().collect();
        events
    }
}

#[cfg(test)]
impl PhysicsWorld {
    /// Move test-fixture hitboxes off Rapier so the same gameplay assertions
    /// exercise both backends in separate feature builds.
    pub(crate) fn promote_hitboxes_for_test(&mut self, owner: impl Fn(EntityId) -> EntityId) {
        let shapes: Vec<_> = self
            .collider_set
            .iter()
            .filter_map(|(_, collider)| {
                if !collider
                    .collision_groups()
                    .memberships
                    .intersects(super::InternalCollisionGroups::HITBOX.bits.into())
                {
                    return None;
                }
                Some((
                    EntityId::from_inner(collider.user_data as u64)?,
                    collider.shared_shape().clone(),
                    *collider.position(),
                ))
            })
            .collect();
        for (entity, shape, pose) in shapes {
            self.remove(entity);
            self.query_hitboxes
                .stage(owner(entity), entity, 0, pose, || shape);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape};
    use cgmath::{Quaternion, point3, vec3};

    fn id(value: u64) -> EntityId {
        EntityId::from_inner(value).unwrap()
    }
    fn rotation() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }
    fn limb(physics: &mut PhysicsWorld, child: EntityId, x: f32) {
        physics.stage_query_hitbox(id(50), child, 0, vec3(x, 0.0, 0.0), rotation(), || {
            SharedShape::cuboid(0.2, 0.8, 0.2)
        });
    }
    fn weapon(physics: &mut PhysicsWorld, group: CollisionGroup) -> RigidBodyHandle {
        let handle = physics.add_dynamic(
            id(20),
            vec3(0.0, 0.0, 0.0),
            rotation(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.2, 0.2, 0.2)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        physics.set_held_item_physical(id(20), group);
        handle
    }
    fn ray(physics: &PhysicsWorld, ignore: Option<EntityId>) -> Option<EntityId> {
        physics
            .ray_cast2(
                point3(-2.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                10.0,
                super::super::InternalCollisionGroups::HITBOX
                    | super::super::InternalCollisionGroups::WORLD,
                ignore,
                true,
            )
            .and_then(|hit| hit.maybe_entity_id)
    }

    #[test]
    fn query_limb_has_no_rapier_body_and_commits_at_the_physics_boundary() {
        let mut physics = PhysicsWorld::new();
        let body_count = physics.rigid_body_set.len();
        let collider_count = physics.collider_set.len();
        limb(&mut physics, id(1), 0.0);
        assert_eq!(
            ray(&physics, None),
            None,
            "pending creation is not committed"
        );
        physics.query_hitboxes.commit();
        assert_eq!(ray(&physics, None), Some(id(1)));
        assert_eq!(ray(&physics, Some(id(1))), None);
        assert_eq!(physics.rigid_body_set.len(), body_count);
        assert_eq!(physics.collider_set.len(), collider_count);
        physics.stage_query_hitbox(id(50), id(1), 0, vec3(0.0, 4.0, 0.0), rotation(), || {
            panic!("geometry must be cached")
        });
        assert_eq!(
            ray(&physics, None),
            Some(id(1)),
            "queued pose must not advance early"
        );
        physics.query_hitboxes.commit();
        assert_eq!(ray(&physics, None), None);
        assert!(physics.get_aabb2(id(1)).unwrap().min.y > 3.0);
        physics.remove(id(50));
        assert!(physics.get_aabb2(id(1)).is_none());
    }

    #[test]
    fn unseen_hidden_or_removed_limbs_stop_participating() {
        let mut physics = PhysicsWorld::new();
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        physics.begin_query_hitbox_frame();
        physics.finish_query_hitbox_frame();
        assert_eq!(ray(&physics, None), None);
        assert!(physics.query_hitboxes.limbs.is_empty());
    }

    #[test]
    fn filters_and_nearest_world_geometry_apply_to_query_only_limbs() {
        let mut physics = PhysicsWorld::new();
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        assert!(
            physics
                .ray_cast2(
                    point3(-2.0, 0.0, 0.0),
                    vec3(1.0, 0.0, 0.0),
                    10.0,
                    super::super::InternalCollisionGroups::WORLD,
                    None,
                    true
                )
                .is_none()
        );
        assert!(
            physics
                .ray_cast2_with_entity_filter(
                    point3(-2.0, 0.0, 0.0),
                    vec3(1.0, 0.0, 0.0),
                    10.0,
                    super::super::InternalCollisionGroups::HITBOX,
                    None,
                    true,
                    &|_| false
                )
                .is_none()
        );
        physics.add_collider(
            id(99),
            ColliderBuilder::cuboid(0.1, 1.0, 1.0)
                .translation(vector![-1.0, 0.0, 0.0])
                .build(),
        );
        let mut player = physics.create_player(vec3(100.0, 100.0, 100.0), id(1000));
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        assert_eq!(ray(&physics, None), Some(id(99)), "wall occludes limb");
    }

    #[test]
    fn overlapping_joints_use_joint_order_not_proxy_allocation_order() {
        let mut physics = PhysicsWorld::new();
        physics.stage_query_hitbox(id(50), id(1), 7, vec3(0.0, 0.0, 0.0), rotation(), || {
            SharedShape::ball(1.0)
        });
        physics.stage_query_hitbox(id(50), id(9), 2, vec3(0.0, 0.0, 0.0), rotation(), || {
            SharedShape::ball(1.0)
        });
        physics.query_hitboxes.commit();
        assert_eq!(ray(&physics, None), Some(id(9)));
    }

    #[test]
    fn melee_sweep_matches_rapier_and_world_occludes_the_limb() {
        let mut physics = PhysicsWorld::new();
        let handle = weapon(&mut physics, CollisionGroup::held_melee());
        limb(&mut physics, id(1), 2.0);
        physics.query_hitboxes.commit();
        let (fraction, hit) =
            physics.held_item_sweep_fraction(handle, &Isometry::identity(), Vector::x(), 4.0);
        let hit = hit.unwrap();
        assert_eq!(hit.limb, id(1));
        assert!(fraction > 0.3 && fraction < 0.5);
        assert!(hit.normal.x > 0.99, "normal must point weapon to limb");
        let mut legacy = PhysicsWorld::new();
        let legacy_weapon = weapon(&mut legacy, CollisionGroup::held_melee());
        legacy.add_kinematic_shared_shape(
            id(1),
            vec3(2.0, 0.0, 0.0),
            rotation(),
            SharedShape::cuboid(0.2, 0.8, 0.2),
            vec3(0.0, 0.0, 0.0),
            CollisionGroup::hitbox(),
            false,
        );
        let mut player = legacy.create_player(vec3(100.0, 100.0, 100.0), id(1000));
        legacy.update(vec3(0.0, 0.0, 0.0), &mut player);
        let (reference, reference_hit) =
            legacy.held_item_sweep_fraction(legacy_weapon, &Isometry::identity(), Vector::x(), 4.0);
        assert!((fraction - reference).abs() < 1e-5);
        assert_eq!(reference_hit.unwrap().limb, hit.limb);
        physics.add_collider(
            id(99),
            ColliderBuilder::cuboid(0.1, 2.0, 2.0)
                .translation(vector![1.0, 0.0, 0.0])
                .build(),
        );
        let mut player = physics.create_player(vec3(100.0, 100.0, 100.0), id(1000));
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let (blocked, hit) =
            physics.held_item_sweep_fraction(handle, &Isometry::identity(), Vector::x(), 4.0);
        assert!(blocked < fraction);
        assert!(hit.is_none(), "wall must prevent a limb damage event");
    }

    #[test]
    fn oblique_extended_limbs_match_direct_shape_queries() {
        // The volume deliberately extends well outside an actor-sized box.
        // Aim through its far end across rotated snapshots, including grazing
        // rays; bounds rejection must agree with direct shape tests.
        let shape = SharedShape::cuboid(3.0, 0.025, 0.025);
        let probe = Ball::new(0.015);
        for sample in 0..120 {
            let angle = sample as f32 * 0.053;
            let pose = Isometry::new(vector![1.0, 0.5, -2.0], vector![0.1, angle, 0.3]);
            let mut registry = QueryHitboxes::default();
            registry.stage(id(50), id(1), 0, pose, || shape.clone());
            registry.commit();
            let origin = pose * point![2.9, (sample % 5) as f32 * 0.012, -1.0];
            let direction = pose.rotation * Vector::z();
            let ray = Ray::new(origin, direction);
            let expected = shape.cast_ray_and_get_normal(&pose, &ray, 2.0, true);
            let actual = registry.cast_ray(&ray, 2.0, &|_| true);
            assert_eq!(actual.is_some(), expected.is_some(), "ray sample {sample}");
            if let (Some((_, actual)), Some(expected)) = (actual, expected) {
                assert!((actual.time_of_impact - expected.time_of_impact).abs() < 1e-5);
            }
            let start = Isometry::translation(origin.x, origin.y, origin.z);
            let expected = query::cast_shapes(
                &pose,
                &Vector::zeros(),
                shape.as_ref(),
                &start,
                &direction,
                &probe,
                ShapeCastOptions {
                    max_time_of_impact: 2.0,
                    target_distance: 0.001,
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                },
            )
            .unwrap();
            let actual = registry.sweep(&start, &direction, &probe, 2.0, 0.001);
            assert_eq!(
                actual.is_some(),
                expected.is_some(),
                "sweep sample {sample}"
            );
            if let (Some((toi, hit)), Some(expected)) = (actual, expected) {
                assert!((toi - expected.time_of_impact).abs() < 1e-5);
                let expected_point = npoint_to_cgvec(pose * expected.witness1);
                assert!(cgmath::InnerSpace::magnitude(hit.point - expected_point) < 1e-5);
            }
        }
    }

    #[test]
    fn placement_checks_query_limbs_and_preserves_ignored_entities() {
        let mut physics = PhysicsWorld::new();
        limb(&mut physics, id(1), 3.0);
        physics.query_hitboxes.commit();
        let size = vec3(0.1, 0.1, 0.1);
        assert!(!physics.placement_box_is_clear(vec3(3.0, 0.0, 0.0), rotation(), size, &[]));
        assert!(physics.placement_box_is_clear(vec3(3.0, 0.0, 0.0), rotation(), size, &[id(1)]));
        assert!(physics.placement_box_is_clear(vec3(0.0, 0.0, 0.0), rotation(), size, &[]));
    }

    #[test]
    fn ignored_collision_pairs_suppress_query_contact_starts() {
        let mut physics = PhysicsWorld::new();
        weapon(&mut physics, CollisionGroup::held_melee());
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        physics.ignore_collision_pairs(id(20), &[id(1)]);
        assert!(physics.query_hitbox_contacts().is_empty());
    }

    #[test]
    fn an_ignored_owner_suppresses_contacts_with_all_of_its_limbs() {
        let mut physics = PhysicsWorld::new();
        weapon(&mut physics, CollisionGroup::held_melee());
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        physics.ignore_collision_pairs(id(20), &[id(50)]);
        assert!(physics.query_hitbox_contacts().is_empty());
    }

    #[test]
    fn resting_overlap_has_enter_edges_and_inert_held_items_do_not_hit_limbs() {
        let mut physics = PhysicsWorld::new();
        weapon(&mut physics, CollisionGroup::held_melee());
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        assert_eq!(physics.query_hitbox_contacts().len(), 1);
        assert!(
            physics.query_hitbox_contacts().is_empty(),
            "persistent overlap is not another start"
        );
        limb(&mut physics, id(1), 4.0);
        physics.query_hitboxes.commit();
        assert!(physics.query_hitbox_contacts().is_empty());
        limb(&mut physics, id(1), 0.0);
        physics.query_hitboxes.commit();
        assert_eq!(
            physics.query_hitbox_contacts().len(),
            1,
            "re-entry gets a new contact"
        );
        physics.remove(id(1));
        assert!(physics.query_hitbox_contacts().is_empty());
        let mut inert = PhysicsWorld::new();
        let gun = weapon(&mut inert, CollisionGroup::held_inert());
        limb(&mut inert, id(1), 0.0);
        inert.query_hitboxes.commit();
        assert!(inert.query_hitbox_contacts().is_empty());
        let (fraction, hit) = inert.held_item_sweep_fraction(
            gun,
            &Isometry::translation(-2.0, 0.0, 0.0),
            Vector::x(),
            4.0,
        );
        assert_eq!(fraction, 1.0);
        assert!(hit.is_none());
    }
}
