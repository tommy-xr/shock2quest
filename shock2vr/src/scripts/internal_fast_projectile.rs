use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3,
    vec3, vec4,
};
use collision::Contains;
use dark::SCALE_FACTOR;

use shipyard::{EntityId, Get, View, World};

use crate::{
    creature::RuntimePropHitBox,
    mission::entity_creator::CreateEntityOptions,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::{RuntimePropProjectileRayOrigin, RuntimePropTransform},
    scripts::{
        Message,
        script_util::{choose_impact_spang, play_impact_sound},
    },
    time::Time,
    util::{get_position_from_transform, get_rotation_from_forward_vector},
};

use super::{Effect, MessagePayload, Script};

/// How far an impact spang is backed off the surface it struck - just enough
/// to keep it out of the world, matching the original engine's hair-width
/// backup (0.01 Dark units). Clearance for the decal riding the spang (the
/// bullet hole) is the decal surface offset's job, applied when its model is
/// created; spawning the spang a visible distance out stacked the two and
/// left the hole hanging off the wall.
const SPANG_SURFACE_BACKUP: f32 = 0.01 / SCALE_FACTOR;

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
        let maybe_camera_origin = world
            .borrow::<View<RuntimePropProjectileRayOrigin>>()
            .ok()
            .and_then(|origins| origins.get(entity_id).ok().map(|origin| origin.0));
        let start_point =
            maybe_camera_origin.unwrap_or(current_position - forward * SCALE_FACTOR * 0.25);
        // A camera-origin ray starts at the player's eye, which is INSIDE the
        // player's own capsule (the eye is the head sphere). Rapier's solid
        // raycast reports a shape containing the origin as a hit at
        // distance 0, so keeping `PLAYER` in the mask would make every flat
        // shot hit the shooter instead of what the crosshair is on.
        let can_hit_player = maybe_camera_origin.is_none();
        let maybe_hit_spot = projectile_ray_cast(
            start_point,
            forward,
            physics,
            distance,
            world,
            can_hit_player,
        );

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
                    position: hit_point + hit_normal * SPANG_SURFACE_BACKUP,
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

/// Whether the shot started inside this entity's own bounds - a weapon pressed
/// against a creature. Its limb proxies can then all lie behind the muzzle,
/// so the coarse capsule hit is the only thing left that means "I shot the
/// creature I am standing in".
fn fired_from_inside(
    physics: &PhysicsWorld,
    start_point: Point3<f32>,
    entity_id: EntityId,
) -> bool {
    physics
        .get_aabb2(entity_id)
        .is_some_and(|bounds| bounds.contains(&start_point))
}

/// How many creature capsules one shot may pass through before it gives up and
/// keeps whatever it last hit. A shot threading two creatures' gaps is already
/// unusual; this only bounds the work.
const MAX_PASS_THROUGH: usize = 4;

fn projectile_ray_cast(
    start_point: Point3<f32>,
    forward: cgmath::Vector3<f32>,
    physics: &PhysicsWorld,
    distance: f32,
    world: &World,
    can_hit_player: bool,
) -> Option<RayCastResult> {
    let player_group = if can_hit_player {
        InternalCollisionGroups::PLAYER
    } else {
        InternalCollisionGroups::empty()
    };
    let coarse_groups = InternalCollisionGroups::ENTITIES
        // Sometimes, the hitbox can stick out past the bounding box...
        // so we should still check for it here
        | InternalCollisionGroups::HITBOX
        | player_group
        | InternalCollisionGroups::SELECTABLE
        | InternalCollisionGroups::WORLD;

    // Creatures whose capsule the shot has already passed through: they had no
    // limb on this line, so they are not what it hit.
    let mut passed_through: Vec<EntityId> = Vec::new();
    for _ in 0..MAX_PASS_THROUGH {
        let is_not_passed_through = |entity_id: EntityId| !passed_through.contains(&entity_id);
        let maybe_hit_spot = physics.ray_cast2_with_entity_filter(
            start_point,
            forward,
            distance,
            coarse_groups,
            None,
            true,
            &is_not_passed_through,
        );

        let Some(hit_spot) = &maybe_hit_spot else {
            return maybe_hit_spot;
        };
        let Some(hit_entity_id) = hit_spot.maybe_entity_id else {
            return maybe_hit_spot;
        };
        // Live proxies, not the creature definition: an authored corpse
        // carries `PropCreature` but is never animated and has none. Reading
        // the definition would refine against hitboxes that do not exist and
        // then pass the shot straight through the body.
        if !crate::creature::has_live_hit_boxes(world, hit_entity_id) {
            return maybe_hit_spot;
        }

        // The coarse hit is a creature's capsule; the shot really landed on
        // whichever of its hitboxes the line crosses.
        let refined_hit = physics.ray_cast2_with_entity_filter(
            start_point,
            forward,
            distance,
            InternalCollisionGroups::HITBOX
                | InternalCollisionGroups::SELECTABLE
                | InternalCollisionGroups::WORLD,
            None,
            true,
            &is_not_passed_through,
        );
        let refined_hits_target_hitbox = refined_hit
            .as_ref()
            .and_then(|hit| hit.maybe_entity_id)
            .is_some_and(|entity_id| hitbox_belongs_to_entity(world, entity_id, hit_entity_id));
        if refined_hits_target_hitbox {
            // Found the limb: that is the hit, and its joint rides along on
            // the damage.
            return refined_hit;
        }
        if fired_from_inside(physics, start_point, hit_entity_id) {
            // Point blank, with the capsule around the muzzle: the proxies may
            // all be behind the origin, and a shot pressed into a creature
            // must not become a terrain impact past it. Keep the coarse hit.
            return maybe_hit_spot;
        }
        // The line crossed the capsule without touching a limb - between the
        // arm and the ribs, say. The capsule is a movement volume, not the
        // creature, so the shot carries on to whatever is behind it.
        passed_through.push(hit_entity_id);
    }
    // Too many creatures deep: take the nearest thing that is left.
    physics.ray_cast2_with_entity_filter(
        start_point,
        forward,
        distance,
        coarse_groups,
        None,
        true,
        &|entity_id: EntityId| !passed_through.contains(&entity_id),
    )
}

#[cfg(test)]
mod tests {
    use super::{hitbox_belongs_to_entity, projectile_ray_cast};
    use crate::creature::{HitBoxType, RuntimePropHitBox};
    use crate::physics::PhysicsWorld;
    use cgmath::{Quaternion, point3, vec3};
    use dark::SCALE_FACTOR;
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
            true,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(player_entity),
            "an AI/turret fast projectile must be able to hit the player's dedicated collider",
        );
    }

    /// The flat crosshair ray starts at the player's eye, which is inside the
    /// player's own capsule (the eye is the head sphere). A solid raycast
    /// reports a containing shape as a distance-0 hit, so the player's own
    /// collider must be excluded or every flat shot hits the shooter.
    #[test]
    fn camera_origin_projectile_shoots_past_the_shooters_own_collider() {
        let mut physics = PhysicsWorld::new();
        let player_entity = EntityId::from_inner(1).unwrap();
        let mut player = physics.create_player(vec3(0.0, 0.0, 0.0), player_entity);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        // The eye: on the capsule axis, inside the player's own collider.
        let body = physics.get_player_translation(&player);
        let eye = point3(
            body.x,
            body.y + crate::PLAYER_EYE_HEIGHT / SCALE_FACTOR,
            body.z,
        );

        let target = EntityId::from_inner(2).unwrap();
        physics.add_kinematic(
            target,
            vec3(0.0, eye.y, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 2.0, 2.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let world = World::new();
        let hit = projectile_ray_cast(eye, vec3(0.0, 0.0, 1.0), &physics, 10.0, &world, false);

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(target),
            "a shot fired from the player's own eye must reach the target, not the shooter",
        );
    }

    /// A creature whose capsule the shot crosses without touching a limb is
    /// not what the shot hit: the capsule is a movement volume, wider than the
    /// creature. The shot carries on to the wall behind it.
    ///
    /// Negative-first: keeping the coarse hit bills the creature for a shot
    /// that passed beside it, with no joint to show for it.
    #[test]
    fn a_shot_through_the_gap_beside_a_limb_passes_the_creature_by() {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        // The creature's capsule spans the line of fire...
        let creature = world.add_entity(crate::creature::RuntimePropHasHitBoxes);
        physics.add_kinematic(
            creature,
            vec3(0.0, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(3.0, 3.0, 1.0),
            crate::physics::CollisionGroup::actor(),
            false,
        );
        // ...but its one hitbox sits well off to the side of it.
        let hit_box = world.add_entity(RuntimePropHitBox {
            parent_entity_id: creature,
            hit_box_type: HitBoxType::Body,
            joint_id: 1,
        });
        physics.add_kinematic(
            hit_box,
            vec3(1.2, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.4, 0.4, 0.4),
            crate::physics::CollisionGroup::hitbox(),
            false,
        );
        // The wall behind.
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 9.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(6.0, 6.0, 1.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );

        // A step so the broad phase sees the fresh bodies. The player is
        // parked far behind the muzzle and never on the line of fire.
        let mut player =
            physics.create_player(vec3(0.0, 0.0, -50.0), EntityId::from_inner(9).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(wall),
            "a shot that crossed the capsule but no limb should reach the wall behind",
        );
    }

    /// ...unless the muzzle is inside the creature. Its limbs can all lie
    /// behind the ray origin, and a shot pressed into a body must not become a
    /// terrain impact past it.
    #[test]
    fn a_point_blank_shot_still_hits_the_creature_it_is_pressed_into() {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        let creature = world.add_entity(crate::creature::RuntimePropHasHitBoxes);
        physics.add_kinematic(
            creature,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(3.0, 3.0, 3.0),
            crate::physics::CollisionGroup::actor(),
            false,
        );
        let hit_box = world.add_entity(RuntimePropHitBox {
            parent_entity_id: creature,
            hit_box_type: HitBoxType::Body,
            joint_id: 1,
        });
        // Behind the muzzle, so the refining cast cannot find it.
        physics.add_kinematic(
            hit_box,
            vec3(0.0, 0.0, -1.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.4, 0.4, 0.4),
            crate::physics::CollisionGroup::hitbox(),
            false,
        );
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 9.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(6.0, 6.0, 1.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );

        let mut player =
            physics.create_player(vec3(0.0, 0.0, -50.0), EntityId::from_inner(9).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(creature),
            "a muzzle inside the creature keeps its coarse hit",
        );
    }

    /// An authored corpse carries `PropCreature` - so the creature definition
    /// says "hitboxes" - but is never animated and has none. Refining against
    /// hitboxes that do not exist would pass every shot straight through the
    /// body.
    #[test]
    fn a_body_with_no_live_hitboxes_still_stops_the_shot() {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        // No `RuntimePropHasHitBoxes`: the proxies were never built.
        let corpse = world.add_entity(());
        physics.add_kinematic(
            corpse,
            vec3(0.0, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(3.0, 3.0, 1.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 9.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(6.0, 6.0, 1.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        let mut player =
            physics.create_player(vec3(0.0, 0.0, -50.0), EntityId::from_inner(9).unwrap());
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(corpse),
            "a body with no hitboxes is hit on its own collider, as it always was",
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
