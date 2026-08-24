use std::collections::HashMap;

use cgmath::{EuclideanSpace, Matrix4, Vector3};
use collision::Aabb;
use dark::{hit_box::HitBoxShape, model::Model, motion::JointId, properties::PropPosition};
use rapier3d::{
    na::Point3 as NaPoint3,
    prelude::{RigidBodyHandle, SharedShape},
};
use shipyard::{Component, EntitiesViewMut, EntityId, IntoIter, IntoWithId, View, ViewMut, World};

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

/// Minimum collider half-extent, so a degenerate joint shape (e.g. a joint with
/// a single skinned vertex) still yields a valid, non-zero box.
const MIN_HALF_EXTENT: f32 = 0.01;

/// The per-joint collision shapes to build hitbox proxies from: the *fitted*
/// shapes (capsule spanning the bone toward its child, box otherwise) shared
/// with the ragdoll, falling back to the raw per-joint vertex AABB for any joint
/// (or model) that has no fitted shape.
///
/// Fitted shapes matter here because these proxies are what melee/projectile
/// damage AND frob/selection raycasts hit: per-joint AABBs cluster around the
/// joint origins and leave the bone segments between them uncovered, which made
/// creatures - corpses especially - fiddly to aim at and to loot.
fn joint_hit_box_shapes(model: &Model) -> HashMap<JointId, HitBoxShape> {
    let mut out: HashMap<JointId, HitBoxShape> = model
        .get_hit_boxes()
        .iter()
        .map(|(joint_id, bbox)| {
            let half = bbox.dim() * 0.5;
            (
                *joint_id,
                HitBoxShape::Cuboid {
                    half_extents: half,
                    center: bbox.center().to_vec(),
                },
            )
        })
        .collect();

    for (joint_id, shape) in model.hit_box_shapes().iter() {
        out.insert(*joint_id, shape.clone());
    }

    out
}

/// Centroid of a fitted shape, in joint-local space.
fn shape_center(shape: &HitBoxShape) -> Vector3<f32> {
    match shape {
        HitBoxShape::Cuboid { center, .. } => *center,
        HitBoxShape::Capsule { a, b, .. } => (*a + *b) * 0.5,
    }
}

/// The rapier collider for a fitted shape, expressed relative to `center` (the
/// proxy body's origin).
fn shape_to_collider(shape: &HitBoxShape, center: Vector3<f32>) -> SharedShape {
    match shape {
        HitBoxShape::Cuboid { half_extents, .. } => SharedShape::cuboid(
            half_extents.x.max(MIN_HALF_EXTENT),
            half_extents.y.max(MIN_HALF_EXTENT),
            half_extents.z.max(MIN_HALF_EXTENT),
        ),
        HitBoxShape::Capsule { a, b, radius } => {
            let a = *a - center;
            let b = *b - center;
            SharedShape::capsule(
                NaPoint3::new(a.x, a.y, a.z),
                NaPoint3::new(b.x, b.y, b.z),
                radius.max(MIN_HALF_EXTENT),
            )
        }
    }
}

pub struct HitBoxManager {
    // Map entity to all the corresponding entities for their joints
    pub hit_boxes: HashMap<EntityId, HashMap<JointId, EntityId>>,
}

impl HitBoxManager {
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
        let joint_updates = {
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

                let hit_boxes = joint_hit_box_shapes(maybe_model.unwrap());
                let creature_type = maybe_creature_type.unwrap();

                let hit_box_map = self.hit_boxes.entry(parent_entity_id).or_insert_with(|| {
                    let mut out_hit_boxes = HashMap::new();

                    for (joint_id, _shape) in hit_boxes.iter() {
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

                    out_hit_boxes
                });

                let mut joint_index = 0;
                for joint_xform in joint_xforms.0 {
                    let maybe_shape = hit_boxes.get(&joint_index);

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
                    let Some(shape) = maybe_shape else {
                        joint_index += 1;
                        continue;
                    };
                    // The proxy body sits at the fitted shape's centroid (as it
                    // used to sit at the joint AABB's center) so a debug aim
                    // point keeps pointing at the middle of the volume; the
                    // collider is then built centered on that origin.
                    let center = shape_center(shape);
                    let joint_xform = xform.0 * joint_xform * Matrix4::from_translation(center);

                    let pos = point3_to_vec3(get_position_from_matrix(&joint_xform));
                    let rotation = get_rotation_from_matrix(&joint_xform);
                    // If there is not a physics entity yet, create one
                    if !id_to_physics.contains_key(hit_box_entry) {
                        let physics_handle = physics.add_kinematic_shape(
                            *hit_box_entry,
                            pos,
                            rotation,
                            shape_to_collider(shape, center),
                            crate::physics::CollisionGroup::hitbox(),
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

            joint_updates
        };

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
