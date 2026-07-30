use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3,
    vec3, vec4,
};
use dark::SCALE_FACTOR;

use shipyard::{EntityId, Get, View, World};

use crate::{
    creature::RuntimePropHitBox,
    mission::entity_creator::CreateEntityOptions,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::{RuntimePropProjectileRayOrigin, RuntimePropTransform},
    scripts::{
        Message,
        ai::ai_util::does_entity_have_hitboxes,
        script_util::{choose_impact_spang, play_impact_sound},
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
        let start_point = world
            .borrow::<View<RuntimePropProjectileRayOrigin>>()
            .ok()
            .and_then(|origins| origins.get(entity_id).ok().map(|origin| origin.0))
            .unwrap_or(current_position - forward * SCALE_FACTOR * 0.25);
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
                        payload: MessagePayload::Damage {
                            amount: 6.0,
                            // The shot's travel direction + hit point seed the
                            // victim's death-ragdoll reaction. Bone is filled
                            // in by the hitbox script when a hitbox was struck.
                            impact: {
                                let travel = hit_point - start_point;
                                if travel.magnitude2() > 1.0e-12 {
                                    Some(crate::scripts::DamageImpact {
                                        direction: travel.normalize(),
                                        point: hit_point.to_vec(),
                                        bone: None,
                                    })
                                } else {
                                    None
                                }
                            },
                        },
                    },
                },
                Effect::DrawDebugLines {
                    lines: vec![(start_point, hit_point, color)],
                },
                Effect::DestroyEntity { entity_id },
                // Impact sound: the projectile's collision schema, tagged with
                // the material of what was hit (flesh thud vs metal clang).
                play_impact_sound(world, entity_id, hit_entity_id, hit_point.to_vec()),
            ];

            // Impact effect, from the projectile's authored spang links
            // (HitSpang matched by victim class, MissSpang fallback).
            if let Some(template_id) = choose_impact_spang(world, entity_id, hit_entity_id) {
                effects.push(Effect::CreateEntity {
                    template_id,
                    position: hit_point + hit_normal * SCALE_FACTOR / 25.0,
                    orientation: get_rotation_from_forward_vector(hit_normal)
                        * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0)),
                    root_transform: Matrix4::identity(),
                    options: CreateEntityOptions {
                        transient_fx: true,
                        ..CreateEntityOptions::default()
                    },
                });
            }

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

fn hitbox_belongs_to_entity(world: &World, hitbox_entity: EntityId, parent: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropHitBox>>()
        .is_ok_and(|hitboxes| {
            hitboxes
                .get(hitbox_entity)
                .is_ok_and(|hitbox| hitbox.parent_entity_id == parent)
        })
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
            | InternalCollisionGroups::PLAYER
            | InternalCollisionGroups::SELECTABLE
            | InternalCollisionGroups::WORLD,
    );

    // If we hit an entity with a hitbox, scan again for the hitbox
    if let Some(hit_spot) = &maybe_hit_spot {
        //let hit_spot = &maybe_hit_spot.unwrap();

        if let Some(hit_entity_id) = &hit_spot.maybe_entity_id {
            if does_entity_have_hitboxes(world, *hit_entity_id) {
                let refined_hit = physics.ray_cast(
                    start_point,
                    forward * distance,
                    InternalCollisionGroups::HITBOX
                        | InternalCollisionGroups::SELECTABLE
                        | InternalCollisionGroups::WORLD,
                );
                // Prefer the authored damage proxy when the ray intersects one.
                // If the coarse creature capsule surrounds the ray origin but
                // its proxies do not cover that exact line, retain the coarse
                // entity hit instead of turning a contact-range shot into a
                // terrain impact beyond the creature.
                let refined_hits_target_hitbox = refined_hit
                    .as_ref()
                    .and_then(|hit| hit.maybe_entity_id)
                    .is_some_and(|entity_id| {
                        hitbox_belongs_to_entity(world, entity_id, *hit_entity_id)
                    });
                if refined_hits_target_hitbox {
                    maybe_hit_spot = refined_hit;
                }
            }
        }
    }

    // TODO: If we missed the entity in the hitbox, we should still raycast through to see if we hit anything else
    // This should be called recursively with some limit (ie, depth=3) to handle those cases

    maybe_hit_spot
}

#[cfg(test)]
mod tests {
    use super::{hitbox_belongs_to_entity, projectile_ray_cast};
    use crate::creature::{HitBoxType, RuntimePropHitBox};
    use crate::physics::PhysicsWorld;
    use cgmath::{point3, vec3};
    use shipyard::{EntityId, World};

    #[test]
    fn projectile_raycast_hits_the_player_collider() {
        let mut physics = PhysicsWorld::new();
        let player_entity = EntityId::from_inner(1).unwrap();
        let mut player = physics.create_player(vec3(0.0, 0.0, 5.0), player_entity);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let world = World::new();
        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            10.0,
            &world,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(player_entity),
            "an AI/turret fast projectile must be able to hit the player's dedicated collider",
        );
    }

    #[test]
    fn refined_hitbox_must_belong_to_the_coarse_creature() {
        let mut world = World::new();
        let coarse_creature = world.add_entity(());
        let overlapping_creature = world.add_entity(());
        let coarse_hitbox = world.add_entity(RuntimePropHitBox {
            parent_entity_id: coarse_creature,
            hit_box_type: HitBoxType::Body,
            joint_id: 1,
        });
        let overlapping_hitbox = world.add_entity(RuntimePropHitBox {
            parent_entity_id: overlapping_creature,
            hit_box_type: HitBoxType::Body,
            joint_id: 1,
        });

        assert!(hitbox_belongs_to_entity(
            &world,
            coarse_hitbox,
            coarse_creature,
        ));
        assert!(!hitbox_belongs_to_entity(
            &world,
            overlapping_hitbox,
            coarse_creature,
        ));
        assert!(!hitbox_belongs_to_entity(
            &world,
            coarse_creature,
            coarse_creature,
        ));
    }
}
