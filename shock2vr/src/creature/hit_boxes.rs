use std::collections::HashMap;

use cgmath::{EuclideanSpace, Matrix4, Vector3, vec3};
use collision::{Aabb, Aabb3, Union};
use dark::{hit_box::HitBoxShape, model::Model, motion::JointId, properties::PropPosition};
use rapier3d::{
    na::Point3 as NaPoint3,
    prelude::{RigidBodyHandle, SharedShape},
};
use shipyard::{
    Component, EntitiesViewMut, EntityId, Get, IntoIter, IntoWithId, View, ViewMut, World,
};

use crate::{
    physics::PhysicsWorld,
    runtime_props::{
        RuntimePropDoNotSerialize, RuntimePropJointTransforms, RuntimePropProxyEntity,
        RuntimePropTransform,
    },
    scripts::ScriptWorld,
    util::{get_position_from_matrix, get_rotation_from_matrix, point3_to_vec3},
};

use super::{get_entity_creature, hit_box_script::HitBoxScript};

/// Marks a creature that has live hitbox proxies, so anything asking "is this
/// struck through limbs?" reads the world rather than the creature definition.
/// The two disagree in shipped data: an authored corpse carries `PropCreature`
/// (so its definition maps hitboxes) but is never animated, so it has none.
#[derive(Component)]
pub struct RuntimePropHasHitBoxes;

/// Whether an entity is one of a creature's hitbox proxies.
pub(crate) fn is_hit_box(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropHitBox>>()
        .is_ok_and(|hit_boxes| hit_boxes.get(entity_id).is_ok())
}

/// Whether an entity is a creature with live hitbox proxies - i.e. whether a
/// blow on it arrives through a limb.
pub(crate) fn has_live_hit_boxes(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropHasHitBoxes>>()
        .is_ok_and(|marked| marked.get(entity_id).is_ok())
}

#[derive(Component)]
pub struct RuntimePropHitBox {
    pub parent_entity_id: EntityId,
    pub hit_box_type: HitBoxType,
    pub joint_id: JointId,
}

#[derive(Clone, Debug)]
pub enum HitBoxType {
    Head,
    Body,
    Limb,
    Extremity,
    #[allow(dead_code)]
    NoDamage,
}

/// Floor for the AABB fallback's half-extents: a degenerate joint AABB (a joint
/// with a single skinned vertex) is zero-sized, and a zero-extent collider makes
/// parry's ray-AABB test overflow. The *fitted* shapes are already clamped where
/// they are produced (`dark::hit_box`).
const FALLBACK_MIN_HALF_EXTENT: f32 = 0.01;

/// The collision shape for one joint: the *fitted* shape (capsule spanning the
/// bone toward its child, box otherwise) shared with the ragdoll, falling back
/// to the raw per-joint vertex AABB for a joint (or model) that has none.
///
/// Fitted shapes matter here because these proxies are what melee/projectile
/// damage AND frob/selection raycasts hit: per-joint AABBs cluster around the
/// joint origins and leave the bone segments between them uncovered, which made
/// creatures - corpses especially - fiddly to aim at and to loot.
fn joint_shape(
    fitted: &HashMap<JointId, HitBoxShape>,
    aabbs: &HashMap<JointId, Aabb3<f32>>,
    joint_id: JointId,
) -> Option<HitBoxShape> {
    if let Some(shape) = fitted.get(&joint_id) {
        return Some(shape.clone());
    }
    let bbox = aabbs.get(&joint_id)?;
    let half = bbox.dim() * 0.5;
    Some(HitBoxShape::Cuboid {
        half_extents: vec3(
            half.x.max(FALLBACK_MIN_HALF_EXTENT),
            half.y.max(FALLBACK_MIN_HALF_EXTENT),
            half.z.max(FALLBACK_MIN_HALF_EXTENT),
        ),
        center: bbox.center().to_vec(),
    })
}

/// Centroid of a fitted shape, in joint-local space.
fn shape_center(shape: &HitBoxShape) -> Vector3<f32> {
    match shape {
        HitBoxShape::Cuboid { center, .. } => *center,
        HitBoxShape::Capsule { a, b, .. } => (*a + *b) * 0.5,
    }
}

/// The rapier collider for a fitted shape, expressed relative to `center` (the
/// proxy body's origin). Non-finite geometry degenerates to a small box rather
/// than reaching parry, whose broad-phase panics on a NaN AABB.
fn shape_to_collider(shape: &HitBoxShape, center: Vector3<f32>) -> SharedShape {
    let min_box = || {
        SharedShape::cuboid(
            FALLBACK_MIN_HALF_EXTENT,
            FALLBACK_MIN_HALF_EXTENT,
            FALLBACK_MIN_HALF_EXTENT,
        )
    };
    match shape {
        HitBoxShape::Cuboid { half_extents, .. } => {
            if !is_finite(*half_extents) {
                return min_box();
            }
            SharedShape::cuboid(
                half_extents.x.max(FALLBACK_MIN_HALF_EXTENT),
                half_extents.y.max(FALLBACK_MIN_HALF_EXTENT),
                half_extents.z.max(FALLBACK_MIN_HALF_EXTENT),
            )
        }
        HitBoxShape::Capsule { a, b, radius } => {
            let a = *a - center;
            let b = *b - center;
            if !is_finite(a) || !is_finite(b) || !radius.is_finite() {
                return min_box();
            }
            SharedShape::capsule(
                NaPoint3::new(a.x, a.y, a.z),
                NaPoint3::new(b.x, b.y, b.z),
                radius.max(FALLBACK_MIN_HALF_EXTENT),
            )
        }
    }
}

fn is_finite(v: Vector3<f32>) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

pub struct HitBoxManager {
    // Map entity to all the corresponding entities for their joints
    pub hit_boxes: HashMap<EntityId, HashMap<JointId, EntityId>>,
}

impl HitBoxManager {
    /// World-space bounds of a creature's hitbox proxies - the volume its
    /// limbs actually occupy in the pose it is drawn in.
    ///
    /// This is what the HUD outline wants: the entity's own collider is a
    /// standing capsule sized from the creature definition, so it frames a
    /// nominal cylinder rather than the creature - and frames the same
    /// cylinder whatever the creature is doing.
    ///
    /// A definition that maps a single `Body` joint (the arachnids and the
    /// Overlord) gets a body-only frame, legs excluded - measured on hydro3's
    /// Baby Arachnids at 0.33-0.58 across, against a 0.40 collider. Neither is
    /// the animal: the shipped creature colliders are their own known problem
    /// (#904). The hitbox is at least measured from the mesh, so it is what is
    /// used, and widening those definitions is the fix worth making.
    ///
    /// The boxes are world AABBs of the *rotated* proxy shapes, so a diagonal
    /// limb contributes a little more than its thickness. The extremes come
    /// from the head, hands and feet, so the inflation is small against the
    /// error it replaces. (It is deliberately read from physics rather than
    /// recomputed from the joint transforms with `dark`'s `joint_box_bounds`:
    /// this is the volume the creature is actually *shot* by, the same proxies
    /// `aim_points` reports.)
    pub fn hit_box_bounds(
        &self,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Option<Aabb3<f32>> {
        let hit_boxes = self.hit_boxes.get(&entity_id)?;
        hit_boxes
            .values()
            .filter_map(|hit_box| physics.get_aabb2(*hit_box))
            .reduce(|acc, bounds| acc.union(&bounds))
    }

    pub fn new() -> HitBoxManager {
        HitBoxManager {
            hit_boxes: HashMap::new(),
        }
    }

    pub fn update(
        &mut self,
        world: &mut World,
        physics: &mut PhysicsWorld,
        script_world: &mut ScriptWorld,
        id_to_model: &HashMap<EntityId, Model>,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        let (joint_updates, marked_parents) = {
            let v_position = world.borrow::<View<PropPosition>>().unwrap();
            let v_runtime_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
            let v_runtime_joints = world.borrow::<View<RuntimePropJointTransforms>>().unwrap();
            let mut v_runtime_hitbox = world.borrow::<ViewMut<RuntimePropHitBox>>().unwrap();
            let mut v_runtime_proxy = world.borrow::<ViewMut<RuntimePropProxyEntity>>().unwrap();
            let mut v_runtime_do_not_serialize = world
                .borrow::<ViewMut<RuntimePropDoNotSerialize>>()
                .unwrap();
            let mut v_entities = world.borrow::<EntitiesViewMut>().unwrap();

            let mut joint_updates = HashMap::new();
            // Applied after this borrow scope: the parents that just gained
            // proxies (see `RuntimePropHasHitBoxes`).
            let mut marked_parents = Vec::new();

            for (parent_entity_id, (_position, xform, joint_xforms)) in
                (&v_position, &v_runtime_transform, &v_runtime_joints)
                    .iter()
                    .with_id()
            {
                let maybe_creature_type = get_entity_creature(world, parent_entity_id);
                if maybe_creature_type.is_none() {
                    continue;
                }

                let maybe_model = id_to_model.get(&parent_entity_id);
                if maybe_model.is_none() {
                    continue;
                }

                // Fitted shapes first, per-joint vertex AABBs as the fallback.
                // Both are `Rc` handles owned by the model - no per-frame
                // rebuild - and both are keyed by the same joints (a joint only
                // gets either when it has skinned vertices), so the AABB map is
                // the authoritative key set for which joints get a proxy.
                let fitted_shapes = maybe_model.unwrap().hit_box_shapes();
                let joint_aabbs = maybe_model.unwrap().get_hit_boxes();
                let creature_type = maybe_creature_type.unwrap();

                let mut built_hit_boxes = false;
                let hit_box_map = self.hit_boxes.entry(parent_entity_id).or_insert_with(|| {
                    let mut out_hit_boxes = HashMap::new();

                    for joint_id in joint_aabbs.keys() {
                        let maybe_hitbox_type = creature_type.get_hitbox_type(*joint_id);
                        if maybe_hitbox_type.is_none() {
                            continue;
                        }

                        let hitbox_type = maybe_hitbox_type.unwrap();

                        // TODO: Create entity through entity collection
                        // let hit_box_entity = v_entities.add_entity(
                        //     &mut v_runtime_hitbox,
                        //     RuntimePropHitBox {
                        //         parent_entity_id: id,
                        //         hit_box_type: hitbox_type,
                        //     },
                        // );
                        let hit_box_entity_id = v_entities.add_entity(
                            &mut v_runtime_hitbox,
                            RuntimePropHitBox {
                                parent_entity_id,
                                hit_box_type: hitbox_type.clone(),
                                joint_id: *joint_id,
                            },
                        );

                        v_entities.add_component(
                            hit_box_entity_id,
                            &mut v_runtime_do_not_serialize,
                            RuntimePropDoNotSerialize,
                        );

                        v_entities.add_component(
                            hit_box_entity_id,
                            &mut v_runtime_do_not_serialize,
                            RuntimePropDoNotSerialize,
                        );

                        v_entities.add_component(
                            hit_box_entity_id,
                            &mut v_runtime_proxy,
                            RuntimePropProxyEntity(parent_entity_id),
                        );

                        script_world.add_entity2(
                            hit_box_entity_id,
                            Box::new(HitBoxScript::new(hitbox_type, parent_entity_id, *joint_id)),
                        );

                        //let hit_box_entity = world.add_entity(());
                        out_hit_boxes.insert(*joint_id, hit_box_entity_id);
                    }

                    built_hit_boxes = !out_hit_boxes.is_empty();
                    out_hit_boxes
                });
                if built_hit_boxes {
                    marked_parents.push(parent_entity_id);
                }

                let mut joint_index = 0;
                for joint_xform in joint_xforms.0 {
                    let maybe_hitbox_type = creature_type.get_hitbox_type(joint_index);

                    if maybe_hitbox_type.is_none() {
                        joint_index += 1;
                        continue;
                    }

                    let maybe_entry = hit_box_map.get(&joint_index);
                    if maybe_entry.is_none() {
                        joint_index += 1;
                        continue;
                    };
                    let hit_box_entry = maybe_entry.unwrap();
                    // Always `Some` for a joint that has a proxy - the proxies
                    // are created from this same key set above.
                    let Some(shape) = joint_shape(&fitted_shapes, &joint_aabbs, joint_index) else {
                        joint_index += 1;
                        continue;
                    };
                    // The proxy body sits at the fitted shape's centroid (as it
                    // used to sit at the joint AABB's center) so a debug aim
                    // point keeps pointing at the middle of the volume; the
                    // collider is then built centered on that origin.
                    let center = shape_center(&shape);
                    let joint_xform = xform.0 * joint_xform * Matrix4::from_translation(center);

                    let pos = point3_to_vec3(get_position_from_matrix(&joint_xform));
                    let rotation = get_rotation_from_matrix(&joint_xform);
                    // If there is not a physics entity yet, create one
                    if !id_to_physics.contains_key(hit_box_entry) {
                        let physics_handle = physics.add_kinematic_shared_shape(
                            *hit_box_entry,
                            pos,
                            rotation,
                            shape_to_collider(&shape, center),
                            vec3(0.0, 0.0, 0.0),
                            crate::physics::CollisionGroup::hitbox(),
                            false,
                        );
                        id_to_physics.insert(*hit_box_entry, physics_handle);
                    } else {
                        //physics.set_transform2(*hit_box_entry, joint_xform);
                        physics.set_position_rotation2(*hit_box_entry, pos, rotation);
                    }

                    joint_updates.insert(*hit_box_entry, joint_xform);

                    joint_index += 1;
                }
            }

            (joint_updates, marked_parents)
        };
        for parent in marked_parents {
            world.add_component(parent, RuntimePropHasHitBoxes);
        }

        for (ent, matrix) in joint_updates {
            world.add_component(ent, RuntimePropTransform(matrix));
            world.add_component(
                ent,
                PropPosition {
                    position: point3_to_vec3(get_position_from_matrix(&matrix)),
                    rotation: get_rotation_from_matrix(&matrix),
                    cell: 0,
                },
            );
        }
    }

    pub(crate) fn remove_entity(
        &mut self,
        entity_id: EntityId,
        world: &mut World,
        script_world: &mut ScriptWorld,
        physics: &mut PhysicsWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        world.remove::<RuntimePropHasHitBoxes>(entity_id);
        if let Some(hitboxes) = self.hit_boxes.remove(&entity_id) {
            for (_, hitbox) in hitboxes {
                physics.remove(hitbox);
                world.delete_entity(hitbox);
                id_to_physics.remove(&hitbox);
                script_world.remove_entity(hitbox);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{CollisionGroup, PhysicsWorld};
    use cgmath::{Quaternion, Zero};

    /// Two hitboxes a body-length apart: the highlight must frame both, not
    /// one of them and not the standing capsule the entity's own collider is.
    #[test]
    fn hit_box_bounds_span_every_hit_box() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let creature = world.add_entity(());
        let mut manager = HitBoxManager::new();
        let mut hit_boxes = HashMap::new();

        for (joint, x) in [(0u32, -1.0), (1, 1.0)] {
            let hit_box = world.add_entity(());
            physics.add_kinematic(
                hit_box,
                vec3(x, 0.0, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
                Vector3::zero(),
                vec3(0.5, 0.5, 0.5),
                CollisionGroup::hitbox(),
                false,
            );
            hit_boxes.insert(joint, hit_box);
        }
        manager.hit_boxes.insert(creature, hit_boxes);

        let bounds = manager
            .hit_box_bounds(&physics, creature)
            .expect("a creature with hitboxes has selection bounds");

        assert!(
            bounds.min.x <= -1.25 && bounds.max.x >= 1.25,
            "got {bounds:?}"
        );
        assert!(
            bounds.min.y >= -0.3 && bounds.max.y <= 0.3,
            "got {bounds:?}"
        );
    }

    /// Anything without hitboxes - every prop, and a posed corpse - has no
    /// hitbox bounds, so the caller keeps using its collider.
    #[test]
    fn hit_box_bounds_are_absent_without_hit_boxes() {
        let mut world = World::new();
        let physics = PhysicsWorld::new();
        let entity_id = world.add_entity(());

        assert!(
            HitBoxManager::new()
                .hit_box_bounds(&physics, entity_id)
                .is_none()
        );
    }

    /// A proxy whose body has gone (mid-teardown) contributes nothing, rather
    /// than a zero-sized box at the world origin that would stretch the
    /// highlight across the level.
    #[test]
    fn hit_box_bounds_ignore_a_proxy_with_no_body() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let creature = world.add_entity(());
        let real = world.add_entity(());
        let phantom = world.add_entity(());
        physics.add_kinematic(
            real,
            vec3(3.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            Vector3::zero(),
            vec3(0.5, 0.5, 0.5),
            CollisionGroup::hitbox(),
            false,
        );
        let mut manager = HitBoxManager::new();
        manager
            .hit_boxes
            .insert(creature, HashMap::from([(0, real), (1, phantom)]));

        let bounds = manager.hit_box_bounds(&physics, creature).unwrap();

        assert!(
            bounds.min.x > 2.0,
            "the phantom must not drag the box to the origin: {bounds:?}"
        );
    }

    /// A creature that maps a single `Body` hitbox (the arachnids, the
    /// Overlord) still gets that hitbox's bounds - it is measured from the
    /// mesh, unlike the capsule beside it.
    #[test]
    fn a_single_hit_box_still_gives_bounds() {
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let creature = world.add_entity(());
        let hit_box = world.add_entity(());
        physics.add_kinematic(
            hit_box,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            Vector3::zero(),
            vec3(0.2, 0.2, 0.2),
            CollisionGroup::hitbox(),
            false,
        );
        let mut manager = HitBoxManager::new();
        manager
            .hit_boxes
            .insert(creature, HashMap::from([(0, hit_box)]));

        let bounds = manager
            .hit_box_bounds(&physics, creature)
            .expect("one hitbox is still bounds");
        assert!(
            (bounds.max.x - bounds.min.x - 0.2).abs() < 0.01,
            "the lone hitbox's own extent, got {bounds:?}"
        );
    }
}
