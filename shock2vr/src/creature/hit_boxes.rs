use std::{collections::HashMap};

use cgmath::{vec3, EuclideanSpace, Matrix4};
use collision::{Aabb, Aabb3};
use dark::{
    model::Model,
    motion::{JointId},
    properties::PropPosition,
};
use rapier3d::prelude::RigidBodyHandle;
use shipyard::{
    Component, EntitiesViewMut, EntityId, IntoIter, IntoWithId, View, ViewMut, World,
};

use crate::{
    physics::PhysicsWorld,
    runtime_props::{RuntimePropDoNotSerialize, RuntimePropJointTransforms, RuntimePropTransform},
    scripts::ScriptWorld,
    util::{
        get_position_from_matrix, get_rotation_from_matrix,
        point3_to_vec3,
    },
};

use super::{get_entity_creature, hit_box_script::HitBoxScript};

#[derive(Component)]
pub struct RuntimePropHitBox {
    pub parent_entity_id: EntityId,
    pub hit_box_type: HitBoxType,
}

#[derive(Clone, Debug)]
pub enum HitBoxType {
    Head,
    Body,
    Limb,
    Extremity,
    NoDamage,
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
            let mut v_runtime_do_not_serialize = world
                .borrow::<ViewMut<RuntimePropDoNotSerialize>>()
                .unwrap();
            let mut v_entities = world.borrow::<EntitiesViewMut>().unwrap();

            let mut joint_updates = HashMap::new();

            for (id, (_position, xform, joint_xforms)) in
                (&v_position, &v_runtime_transform, &v_runtime_joints)
                    .iter()
                    .with_id()
            {
                let maybe_creature_type = get_entity_creature(world, id);
                if maybe_creature_type.is_none() {
                    continue;
                }

                let maybe_model = id_to_model.get(&id);
                if maybe_model.is_none() {
                    continue;
                }

                let hit_boxes = maybe_model.unwrap().get_hit_boxes();
                let creature_type = maybe_creature_type.unwrap();

                let hit_box_map = self.hit_boxes.entry(id).or_insert_with(|| {
                    let mut out_hit_boxes = HashMap::new();

                    for (joint_id, _bbox) in hit_boxes.iter() {
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
                        let hit_box_entity = v_entities.add_entity(
                            &mut v_runtime_hitbox,
                            RuntimePropHitBox {
                                parent_entity_id: id,
                                hit_box_type: hitbox_type.clone(),
                            },
                        );

                        v_entities.add_component(
                            hit_box_entity,
                            &mut v_runtime_do_not_serialize,
                            RuntimePropDoNotSerialize,
                        );

                        script_world.add_entity2(
                            hit_box_entity,
                            Box::new(HitBoxScript::new(hitbox_type, id, *joint_id)),
                        );

                        //let hit_box_entity = world.add_entity(());
                        out_hit_boxes.insert(*joint_id, hit_box_entity);
                    }

                    out_hit_boxes
                });

                let mut joint_index = 0;
                for joint_xform in joint_xforms.0 {
                    let bbox = hit_boxes
                        .get(&joint_index).copied()
                        .unwrap_or(Aabb3::zero());
                    let sizes = bbox.dim() * 1.0;

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
                    let joint_xform =
                        xform.0 * joint_xform * Matrix4::from_translation(bbox.center().to_vec());
                    //* Matrix4::from_nonuniform_scale(sizes.x, sizes.y, sizes.z);

                    let pos = point3_to_vec3(get_position_from_matrix(&joint_xform));
                    let rotation = get_rotation_from_matrix(&joint_xform);
                    // If there is not a physics entity yet, create one
                    if !id_to_physics.contains_key(hit_box_entry) {
                        let physics_handle = physics.add_kinematic(
                            *hit_box_entry,
                            pos,
                            rotation,
                            vec3(0.0, 0.0, 0.0),
                            vec3(sizes.x, sizes.y, sizes.z),
                            crate::physics::CollisionGroup::hitbox(),
                            false,
                        );
                        id_to_physics.insert(*hit_box_entry, physics_handle);
                    } else {
                        //physics.set_transform2(*hit_box_entry, joint_xform);
                        physics.set_position_rotation2(*hit_box_entry, pos, rotation);
                    }

                    // let hit_box_entity =
                    //     world.add_component(*hit_box_entry, RuntimePropTransform(xform.0));

                    //v_entities.add_component(*hit_box_entry, &mut v__mut_runtime_transform, RuntimePropTransform(xform.0));

                    joint_updates.insert(*hit_box_entry, joint_xform);

                    // TODO: Get pos, facing from transform
                    // let pos = crate::util::get_position_from_transform(
                    //     world,
                    //     parent_entity,
                    //     offset,
                    // );
                    // let facing = get_rotation_from_transform(world, parent_entity);

                    // TODO:
                    // world.add_component(
                    //     hit_box_entity,
                    //     PropPosition {
                    //         position: pos,
                    //         rotation: facing,
                    //         cell: 0,
                    //     },
                    // );
                    // v_entities.add_component(
                    //     *hit_box_entry,
                    //     &mut v_runtime_transform,
                    //     RuntimePropTransform(joint_xform),
                    // );
                    //world.add_component(*hit_box_entity, RuntimePropTransform(joint_xform));

                    // Debug render stuff:
                    // let hitbox_type = maybe_hitbox_type.unwrap();
                    // let color = match hitbox_type {
                    //     HitBoxType::Head => vec3(1.0, 0.0, 0.0),
                    //     HitBoxType::Body => vec3(0.0, 1.0, 0.0),
                    //     HitBoxType::Limb => vec3(0.0, 0.0, 1.0),
                    //     HitBoxType::Extremity => vec3(0.0, 0.0, 0.0),
                    //     HitBoxType::NoDamage => vec3(1.0, 1.0, 1.0),
                    // };

                    // let player_mat = engine::scene::color_material::create(color);
                    // let mut other =
                    //     SceneObject::new(player_mat, Box::new(engine::scene::cube::create()));
                    // // Set joint transform
                    // other.set_transform(
                    //     xform.0
                    //         * joint_xform
                    //         * Matrix4::from_translation(bbox.center().to_vec())
                    //         * Matrix4::from_nonuniform_scale(sizes.x, sizes.y, sizes.z),
                    // );
                    // scene.push(other);

                    joint_index += 1;
                }
            }

            // for (entity, animation_player) in id_to_animation_player {
            //     let mut hit_boxes = HashMap::new();
            //     if hit_boxes.contains_key(entity) {
            //         // Update hit box positions
            //     } else {
            //         // Insert hitboxes
            //         for (joint_id, _) in animation_player.skeleton.joints.iter() {
            //             let joint_entity = world
            //                 .try_borrow::<HashMap<JointId, EntityId>>()
            //                 .unwrap()
            //                 .get(&joint_id)
            //                 .unwrap();
            //             hit_boxes.insert(*joint_id, *joint_entity);
            //         }
            //         self.hit_boxes.insert(*entity, hit_boxes);
            //     }
            // }
            //panic!("update hitboxes!");
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
