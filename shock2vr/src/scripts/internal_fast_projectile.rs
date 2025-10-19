use cgmath::{
    vec3, vec4, Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3,
};
use dark::{properties::Link, SCALE_FACTOR};

use shipyard::{EntityId, Get, View, World};

use crate::{
    creature::RuntimePropHitBox,
    mission::entity_creator::CreateEntityOptions,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::RuntimePropTransform,
    scripts::{
        ai::ai_util::does_entity_have_hitboxes, script_util::get_first_link_with_template_and_data,
        Message,
    },
    time::Time,
    util::{get_position_from_transform, get_rotation_from_forward_vector},
};

use super::{Effect, MessagePayload, Script};

pub struct InternalFastProjectileScript {
    velocity: Vector3<f32>,
}
impl InternalFastProjectileScript {
    pub fn new(velocity: Vector3<f32>) -> InternalFastProjectileScript {
        InternalFastProjectileScript { velocity }
    }
}

impl Script for InternalFastProjectileScript {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        //let speed = 100.0;
        let distance = 1000.0;
        //let distance = speed * time.elapsed.as_secs_f32();

        let v_runtime_prop_hitbox = world.borrow::<View<RuntimePropHitBox>>().unwrap();
        let v_runtime_prop_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
        let _xform = v_runtime_prop_transform.get(entity_id).unwrap().0;

        let current_position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
        // let forward = xform.transform_vector(vec3(0.0, 0.0, -1.0));
        let forward = self.velocity.normalize();
        let start_point = current_position - forward * SCALE_FACTOR * 0.25;
        let maybe_hit_spot = projectile_ray_cast(start_point, forward, physics, distance, world);

        if let Some(RayCastResult {
            hit_point,
            maybe_entity_id: Some(hit_entity_id),
            hit_normal,
            maybe_rigid_body_handle: _,
            is_sensor: _,
        }) = maybe_hit_spot
        {
            // Effect::SetPosition {
            //     entity_id,
            //     position: hit_result.hit_point.to_vec(),
            // }
            let did_hit_hitbox = v_runtime_prop_hitbox.get(hit_entity_id).is_ok();
            let color = if did_hit_hitbox {
                vec4(1.0, 0.0, 0.0, 1.0)
            } else {
                vec4(0.0, 1.0, 0.0, 1.0)
            };

            let mut effects = vec![
                Effect::Send {
                    msg: Message {
                        to: hit_entity_id,
                        // TODO: Properly calculate damage
                        payload: MessagePayload::Damage { amount: 6.0 },
                    },
                },
                Effect::DrawDebugLines {
                    lines: vec![(start_point, hit_point, color)],
                },
                Effect::DestroyEntity { entity_id },
            ];

            let miss_spang = get_first_link_with_template_and_data(world, entity_id, |link| {
                if matches!(link, Link::MissSpang) {
                    Some(())
                } else {
                    None
                }
            });

            if miss_spang.is_some() {
                // effects.push(Effect::CreateEntity {
                //     //template_id: miss_spang.unwrap().0,
                //     template_id: -2653, // assault flash
                //     position: hit_point.to_vec() + hit_normal * 0.1,
                //     orientation: Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0)),
                //     velocity: vec3(0.0, 0.0, 0.0),
                //     root_transform: Matrix4::identity(),
                // });
            };

            // TEMPORARY: Just so some effect until the blood is sorted
            let template_id = if did_hit_hitbox {
                -2653 /* assault flash */
            } else {
                -3544 /* bullet hit */
            };
            effects.push(Effect::CreateEntity {
                template_id,
                position: hit_point + hit_normal * SCALE_FACTOR / 25.0,
                orientation: get_rotation_from_forward_vector(hit_normal)
                    * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0)),
                root_transform: Matrix4::identity(),
                options: CreateEntityOptions::default(),
            });

            Effect::combine(effects)
        } else {
            Effect::combine(vec![
                Effect::DrawDebugLines {
                    lines: vec![(
                        start_point,
                        start_point + forward * distance,
                        vec4(0.0, 1.0, 0.0, 1.0),
                    )],
                },
                Effect::DestroyEntity { entity_id },
            ])
        }
    }
    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}

fn projectile_ray_cast(
    start_point: Point3<f32>,
    forward: cgmath::Vector3<f32>,
    physics: &PhysicsWorld,
    distance: f32,
    world: &World,
) -> Option<RayCastResult> {
    let mut maybe_hit_spot = physics.ray_cast(
        start_point,
        forward * distance,
        InternalCollisionGroups::ENTITY
            // Sometimes, the hitbox can stick out past the bounding box...
            // so we should still check for it here
            | InternalCollisionGroups::HITBOX 
            | InternalCollisionGroups::SELECTABLE
            | InternalCollisionGroups::WORLD,
    );

    // If we hit an entity with a hitbox, scan again for the hitbox
    if let Some(hit_spot) = &maybe_hit_spot {
        //let hit_spot = &maybe_hit_spot.unwrap();

        if let Some(hit_entity_id) = &hit_spot.maybe_entity_id {
            if does_entity_have_hitboxes(world, *hit_entity_id) {
                maybe_hit_spot = physics.ray_cast(
                    start_point,
                    forward * distance,
                    InternalCollisionGroups::HITBOX
                        | InternalCollisionGroups::SELECTABLE
                        | InternalCollisionGroups::WORLD,
                );
            }
        }
    }

    // TODO: If we missed the entity in the hitbox, we should still raycast through to see if we hit anything else
    // This should be called recursively with some limit (ie, depth=3) to handle those cases

    maybe_hit_spot
}
