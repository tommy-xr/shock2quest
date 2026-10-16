use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, One, Point3, Quaternion, Rotation3, SquareMatrix,
    Vector3, vec3, vec4,
};
use collision::Contains;
use dark::{SCALE_FACTOR, properties::PropPosition};

use shipyard::{EntityId, Get, View, World};

use crate::{
    creature::RuntimePropHitBox,
    mission::entity_creator::CreateEntityOptions,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::{
        RuntimePropPlayerFiredProjectile, RuntimePropProjectileLauncher,
        RuntimePropProjectileRayOrigin,
    },
    scripts::script_util::{choose_impact_spang, play_impact_sound, projectile_contact_effects},
    time::Time,
    util::{get_position_from_transform, get_rotation_from_forward_vector},
};

use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
};
use serde::{Deserialize, Serialize};

/// How far an impact spang is backed off the surface it struck - just enough
/// to keep it out of the world, matching the original engine's hair-width
/// backup (0.01 Dark units). Clearance for the decal riding the spang (the
/// bullet hole) is the decal surface offset's job, applied when its model is
/// created; spawning the spang a visible distance out stacked the two and
/// left the hole hanging off the wall.
const SPANG_SURFACE_BACKUP: f32 = 0.01 / SCALE_FACTOR;

pub struct InternalFastProjectileScript {
    velocity: Vector3<f32>,
    /// `Some` for a shot that travels, decided once when it was fired; `None`
    /// resolves its whole range in one raycast on its first update.
    flight: Option<Flight>,
}

/// A traveling shot's progress along its line of fire.
#[derive(Clone, Serialize, Deserialize)]
struct Flight {
    /// World units per world second.
    speed: f32,
    /// Where the line starts; fixed on the first step.
    origin: Option<Point3<f32>>,
    /// Where the shot entity was created, which a VR line starts behind.
    spawn: Point3<f32>,
    traveled: f32,
    age: f32,
}

const SCRIPT_STATE_KEY: &str = "shock2vr.internal_fast_projectile";

/// Saved so a shot in flight resumes mid-air after a load.
#[derive(Serialize, Deserialize)]
struct FastProjectileState {
    velocity: Vector3<f32>,
    flight: Option<Flight>,
}

/// World seconds a traveling shot may fly before it is dropped.
const MAX_FLIGHT_SECONDS: f32 = 10.0;

impl InternalFastProjectileScript {
    pub fn new(velocity: Vector3<f32>) -> InternalFastProjectileScript {
        InternalFastProjectileScript {
            velocity,
            flight: None,
        }
    }

    /// A shot that advances `speed` world units per world second, sweeping
    /// each step for hits, instead of resolving its range at once.
    pub fn traveling(velocity: Vector3<f32>, speed: f32) -> InternalFastProjectileScript {
        InternalFastProjectileScript {
            velocity,
            flight: Some(Flight {
                speed,
                origin: None,
                spawn: Point3::origin(),
                traveled: 0.0,
                age: 0.0,
            }),
        }
    }

    /// Picks the mode at launch: while the world is slowed a shot travels at
    /// its launch speed (world units/s in `velocity`) times `shot_speed`, so it
    /// can be seen and dodged; at full speed it stays hitscan. A shot in
    /// flight keeps its mode when the scale changes.
    pub fn for_launch(
        velocity: Vector3<f32>,
        world_time_scale: f32,
        shot_speed: f32,
    ) -> InternalFastProjectileScript {
        if world_time_scale < 1.0 {
            Self::traveling(velocity, velocity.magnitude() * shot_speed)
        } else {
            Self::new(velocity)
        }
    }

    #[cfg(test)]
    fn is_traveling(&self) -> bool {
        self.flight.is_some()
    }
}

impl Script for InternalFastProjectileScript {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let forward = self.velocity.normalize();
        let current_position = get_position_from_transform(world, entity_id, vec3(0.0, 0.0, 0.0));
        let launch_point = || {
            world
                .borrow::<View<RuntimePropProjectileRayOrigin>>()
                .ok()
                .and_then(|origins| origins.get(entity_id).ok().map(|origin| origin.0))
                .unwrap_or(current_position - forward * SCALE_FACTOR * 0.25)
        };
        // A player-fired shot must never hit the player. Rapier's solid raycast
        // reports a shape containing the ray origin as a hit at distance 0, and
        // the ray starts inside the player's own capsule in both presentations:
        // flat rays start at the eye (the head sphere), and a VR ray starts at
        // the weapon's muzzle, which is inside the capsule whenever the weapon
        // is held in close to the body. Keeping `PLAYER` in the mask makes the
        // shot spang on the shooter before it travels. AI and turret
        // projectiles carry no marker and can still hit the player.
        let can_hit_player = !is_player_fired(world, entity_id);
        let launcher = world
            .borrow::<View<RuntimePropProjectileLauncher>>()
            .ok()
            .and_then(|launchers| launchers.get(entity_id).ok().map(|launcher| launcher.0));
        let cast = |start: Point3<f32>, muzzle: Point3<f32>, distance: f32| {
            projectile_segment_cast(
                start,
                muzzle,
                forward,
                physics,
                distance,
                world,
                can_hit_player,
                launcher,
            )
        };

        let Some(flight) = &mut self.flight else {
            let distance = 1000.0;
            let start_point = launch_point();
            return match cast(start_point, start_point, distance) {
                Some(hit) if hit.maybe_entity_id.is_some() => {
                    hit_effects(world, physics, entity_id, start_point, hit)
                }
                _ => Effect::combine(vec![
                    Effect::DrawDebugLines {
                        lines: vec![(
                            start_point,
                            start_point + forward * distance,
                            vec4(0.0, 1.0, 0.0, 1.0),
                        )],
                    },
                    Effect::DestroyEntity { entity_id },
                ]),
            };
        };

        // Traveling: sweep only this world step's segment of the line. The
        // first segment starts where a hitscan shot would (the eye in flat),
        // so point-blank hits land the same.
        let dt = time.elapsed.as_secs_f32();
        if dt <= 0.0 {
            return Effect::NoEffect;
        }
        let origin = *flight.origin.get_or_insert_with(|| {
            flight.spawn = current_position;
            launch_point()
        });
        let start = origin + forward * flight.traveled;
        let step = (flight.speed * dt).min(MAX_RANGE - flight.traveled);
        flight.age += dt;
        match cast(start, origin, step) {
            // Anything struck stops the shot, even a collider with no entity.
            Some(hit) => Effect::combine(vec![
                Effect::ProjectileTrailEnd {
                    entity_id,
                    point: hit.hit_point.to_vec(),
                },
                hit_effects(world, physics, entity_id, origin, hit),
            ]),
            None => {
                flight.traveled += step;
                let end = start + forward * step;
                if flight.traveled >= MAX_RANGE || flight.age >= MAX_FLIGHT_SECONDS {
                    return Effect::DestroyEntity { entity_id };
                }
                let debug_line = Effect::DrawDebugLines {
                    lines: vec![(start, end, vec4(0.0, 1.0, 0.0, 1.0))],
                };
                // A VR line starts behind the muzzle; stay put until it passes
                // the spawn point rather than jump back into the gun.
                if (end - flight.spawn).dot(forward) <= 0.0 {
                    return debug_line;
                }
                let rotation = world
                    .borrow::<View<PropPosition>>()
                    .ok()
                    .and_then(|positions| positions.get(entity_id).ok().map(|p| p.rotation))
                    .unwrap_or_else(Quaternion::one);
                Effect::combine(vec![
                    debug_line,
                    Effect::SetPositionRotation {
                        entity_id,
                        position: end.to_vec(),
                        rotation,
                    },
                ])
            }
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

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &FastProjectileState {
                velocity: self.velocity,
                flight: self.flight.clone(),
            },
            SCRIPT_STATE_KEY,
        )
    }

    /// Replaces whatever mode the loading build picked with the saved one.
    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: FastProjectileState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.velocity = restored.velocity;
        self.flight = restored.flight;
        Ok(())
    }
}

/// Everything a shot does when it strikes `hit`, fired along a line from
/// `start_point`: damage, the victim's collision notice, impact sound and
/// spang, and the shot's own removal (only the removal if no entity was hit).
fn hit_effects(
    world: &World,
    physics: &PhysicsWorld,
    entity_id: EntityId,
    start_point: Point3<f32>,
    hit: RayCastResult,
) -> Effect {
    let RayCastResult {
        hit_point,
        maybe_entity_id: Some(hit_entity_id),
        hit_normal,
        maybe_rigid_body_handle: _,
        is_sensor: _,
        surface_material,
    } = hit
    else {
        return Effect::DestroyEntity { entity_id };
    };
    let did_hit_hitbox = world
        .borrow::<View<RuntimePropHitBox>>()
        .is_ok_and(|hit_boxes| hit_boxes.get(hit_entity_id).is_ok());
    let color = if did_hit_hitbox {
        vec4(1.0, 0.0, 0.0, 1.0)
    } else {
        vec4(0.0, 1.0, 0.0, 1.0)
    };

    let mut effects = vec![
        projectile_contact_effects(world, entity_id, hit_entity_id, {
            let travel = hit_point - start_point;
            (travel.magnitude2() > 1.0e-12).then(|| crate::scripts::DamageImpact {
                direction: travel.normalize(),
                point: hit_point.to_vec(),
                bone: None,
            })
        }),
        // Ray projectiles bypass Rapier's collision-message dispatch.
        // Notify the owning victim even when its receptrons absorb all
        // damage: collision-driven shield feedback still needs the hit.
        Effect::Send {
            msg: Message {
                to: crate::util::resolve_proxy_entity(world, hit_entity_id),
                payload: MessagePayload::Collided {
                    with: entity_id,
                    contact: Some(crate::physics::CollisionContact {
                        point: hit_point.to_vec(),
                        normal: hit_normal,
                        closing_speed: None,
                        surface_material,
                    }),
                },
            },
        },
        Effect::DrawDebugLines {
            lines: vec![(start_point, hit_point, color)],
        },
        Effect::DestroyEntity { entity_id },
        // Impact sound: the projectile's collision schema, tagged with
        // the material of what was hit (flesh thud vs metal clang).
        play_impact_sound(
            world,
            entity_id,
            hit_entity_id,
            hit_point.to_vec(),
            surface_material.and_then(|material| physics.surface_material_name(material)),
        ),
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
                impact_appearance: super::script_util::ballistic_impact_appearance(
                    world,
                    entity_id,
                    hit_entity_id,
                    surface_material.and_then(|id| physics.surface_material_name(id)),
                ),
                ..CreateEntityOptions::default()
            },
        });
    }

    Effect::combine(effects)
}

/// Whether this projectile came out of a weapon the player fired (every
/// `weapon_script::create_projectile` shot - flat and VR alike).
fn is_player_fired(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropPlayerFiredProjectile>>()
        .is_ok_and(|fired| fired.contains(entity_id))
}

/// Whether a collider is one of this creature's own hitboxes. Read from the
/// proxy's `parent_entity_id` - the fact that says whose limb this is - rather
/// than from the generic proxy link `resolve_proxy_entity` follows.
fn hitbox_belongs_to_entity(world: &World, hitbox_entity: EntityId, parent: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropHitBox>>()
        .ok()
        .and_then(|hit_boxes| {
            hit_boxes
                .get(hitbox_entity)
                .ok()
                .map(|hb| hb.parent_entity_id)
        })
        == Some(parent)
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

/// How far a shot reaches, in world units. The shipped value: `ray_cast`
/// normalizes its direction and caps the cast at 100, so the caller's nominal
/// 1000 never applied. Kept explicit here so the range stays what it has
/// always been rather than becoming ten times longer by accident.
const MAX_RANGE: f32 = 100.0;

/// A whole shot fired from `start_point`, for tests.
#[cfg(test)]
fn projectile_ray_cast(
    start_point: Point3<f32>,
    forward: cgmath::Vector3<f32>,
    physics: &PhysicsWorld,
    distance: f32,
    world: &World,
    can_hit_player: bool,
    launcher: Option<EntityId>,
) -> Option<RayCastResult> {
    projectile_segment_cast(
        start_point,
        start_point,
        forward,
        physics,
        distance,
        world,
        can_hit_player,
        launcher,
    )
}

/// [`projectile_ray_cast`] over one segment of a shot's line. `muzzle` is
/// where the shot was fired from, which decides a point-blank hit: a later
/// segment starting inside a creature it entered through a gap has not been
/// fired from inside it.
#[allow(clippy::too_many_arguments)]
fn projectile_segment_cast(
    start_point: Point3<f32>,
    muzzle: Point3<f32>,
    forward: cgmath::Vector3<f32>,
    physics: &PhysicsWorld,
    distance: f32,
    world: &World,
    can_hit_player: bool,
    // The AI or turret that fired the shot (see `RuntimePropProjectileLauncher`).
    launcher: Option<EntityId>,
) -> Option<RayCastResult> {
    let player_group = if can_hit_player {
        InternalCollisionGroups::PLAYER
    } else {
        InternalCollisionGroups::empty()
    };
    let distance = distance.min(MAX_RANGE);
    let coarse_groups = InternalCollisionGroups::ENTITIES
        // Sometimes, the hitbox can stick out past the bounding box...
        // so we should still check for it here
        | InternalCollisionGroups::HITBOX
        | player_group
        | InternalCollisionGroups::SELECTABLE
        | InternalCollisionGroups::WORLD;

    // Creatures whose *capsule* the shot has already passed through: it found
    // no limb of theirs on this line. Their proxies stay in play - if a later
    // pass finds one, that is a genuine limb hit and wins.
    let mut passed_through: Vec<EntityId> = Vec::new();
    for pass in 0..=MAX_PASS_THROUGH {
        let is_not_passed_through = |entity_id: EntityId| {
            !passed_through.contains(&entity_id)
                && !launcher.is_some_and(|launcher| {
                    entity_id == launcher || hitbox_belongs_to_entity(world, entity_id, launcher)
                })
        };
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
        // Out of passes: take the nearest thing that is left, capsule or not.
        if pass == MAX_PASS_THROUGH {
            return maybe_hit_spot;
        }
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
        if !crate::creature::hit_boxes_cover_body(world, hit_entity_id) {
            // The creature's hitboxes do not stand in for its body: the
            // Overlord maps a single `Body` joint, so its limbs have no proxy
            // at all. Passing through would make whole bands of it
            // unshootable, so the capsule keeps the shot (with no joint to
            // report). Widening that definition is the fix that would let
            // it join the rule above.
            return maybe_hit_spot;
        }
        if fired_from_inside(physics, muzzle, hit_entity_id) {
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
    None
}

#[cfg(test)]
mod tests {
    use super::{hitbox_belongs_to_entity, is_player_fired, projectile_ray_cast};
    use crate::creature::{HitBoxType, RuntimePropHitBox};
    use crate::physics::PhysicsWorld;
    use crate::runtime_props::RuntimePropPlayerFiredProjectile;
    use cgmath::{Quaternion, point3, vec3};
    use dark::properties::PropCreature;
    use shipyard::{EntityId, World};

    #[test]
    fn absorbed_fast_projectile_still_notifies_the_struck_script() {
        use crate::scripts::{Effect, MessagePayload, Script};
        use cgmath::{Matrix4, SquareMatrix};
        use dark::properties::{
            Link, Links, PropTemplateId, ReceptronEffect, ReceptronOptions, ToLink,
        };
        let mut world = World::new();
        let victim = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: -50,
                to_entity_id: None,
                link: Link::Receptron(ReceptronOptions {
                    order: 70,
                    effect: ReceptronEffect::Abort,
                }),
            }],
        });
        let projectile = world.add_entity((
            PropTemplateId { template_id: -40 },
            crate::runtime_props::RuntimePropTransform(Matrix4::identity()),
            RuntimePropPlayerFiredProjectile,
        ));
        world.add_unique(crate::mission::stim_response::GlobalContactStims(
            std::collections::HashMap::from([(-40, vec![(-50, 10.0)])]),
        ));
        let mut physics = PhysicsWorld::new();
        let player_id = world.add_entity(());
        let mut player = physics.create_player(vec3(10.0, 0.0, 0.0), player_id);
        physics.add_kinematic(
            victim,
            vec3(0.0, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 2.0, 2.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let effects = Effect::flatten(vec![
            super::InternalFastProjectileScript::new(vec3(0.0, 0.0, 1.0)).update(
                projectile,
                &world,
                &physics,
                &crate::time::Time::default(),
            ),
        ]);
        assert!(!effects.iter().any(|effect| matches!(effect, Effect::Send { msg } if matches!(msg.payload, MessagePayload::Damage { .. }))));
        assert!(effects.iter().any(|effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == projectile)));
        assert!(effects.iter().any(|effect| matches!(effect, Effect::Send { msg } if msg.to == victim && matches!(msg.payload, MessagePayload::Collided { with, contact: Some(_) } if with == projectile))), "absorbed hits must still reach collision-driven feedback scripts");
    }

    /// A traveling shot fired along +Z from the origin at a wall thinner than
    /// one step, stepped at `speed` world units/s in 60 Hz world ticks.
    /// Returns, per tick, the effects it emitted.
    fn fly_at_thin_wall(
        speed: f32,
        wall_z: f32,
        ticks: usize,
    ) -> (Vec<Vec<super::Effect>>, EntityId) {
        fly_at_thin_wall_reloading(speed, wall_z, ticks, None)
    }

    /// [`fly_at_thin_wall`], saving the script after `reload_after` ticks and
    /// restoring it into a fresh hitscan script aimed elsewhere, as a load
    /// at full world speed would build it.
    fn fly_at_thin_wall_reloading(
        speed: f32,
        wall_z: f32,
        ticks: usize,
        reload_after: Option<usize>,
    ) -> (Vec<Vec<super::Effect>>, EntityId) {
        use crate::runtime_props::RuntimePropProjectileRayOrigin;
        use crate::scripts::{Effect, Script};
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let player_id = world.add_entity(());
        let mut player = physics.create_player(vec3(0.0, 0.0, -20.0), player_id);
        let wall = world.add_entity(());
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, wall_z),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(4.0, 4.0, 0.05),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let projectile = world.add_entity((
            RuntimePropProjectileRayOrigin(point3(0.0, 0.0, 0.0)),
            RuntimePropPlayerFiredProjectile,
        ));
        let mut script = super::InternalFastProjectileScript::traveling(vec3(0.0, 0.0, 1.0), speed);
        let tick = crate::time::Time {
            elapsed: std::time::Duration::from_secs_f32(1.0 / 60.0),
            total: std::time::Duration::ZERO,
        };
        let mut per_tick = Vec::new();
        for i in 0..ticks {
            if reload_after == Some(i) {
                let saved = script.save_state().unwrap();
                script = super::InternalFastProjectileScript::new(vec3(1.0, 0.0, 0.0));
                let ids = std::collections::HashMap::new();
                script
                    .restore_state(&saved, &crate::scripts::ScriptRestoreContext::new(&ids))
                    .unwrap();
            }
            let effects = Effect::flatten(vec![script.update(projectile, &world, &physics, &tick)]);
            let done = effects
                .iter()
                .any(|effect| matches!(effect, Effect::DestroyEntity { .. }));
            per_tick.push(effects);
            if done {
                break;
            }
        }
        (per_tick, wall)
    }

    fn set_position_z(effects: &[super::Effect]) -> Option<f32> {
        effects.iter().find_map(|effect| match effect {
            super::Effect::SetPositionRotation { position, .. } => Some(position.z),
            _ => None,
        })
    }

    #[test]
    fn a_traveling_shot_advances_speed_times_dt_each_world_tick() {
        // 30 wu/s at 60 Hz: 0.5 wu per tick, wall far away.
        let (per_tick, _) = fly_at_thin_wall(30.0, 50.0, 4);
        let zs: Vec<f32> = per_tick.iter().filter_map(|e| set_position_z(e)).collect();
        assert_eq!(zs.len(), 4, "no hit yet, so every tick moves the shot");
        for (i, z) in zs.iter().enumerate() {
            assert!((z - 0.5 * (i + 1) as f32).abs() < 1e-4, "tick {i}: z = {z}");
        }
    }

    #[test]
    fn a_traveling_shot_hits_a_thin_wall_on_the_tick_it_crosses_it_once() {
        use crate::scripts::{Effect, MessagePayload};
        // 60 wu/s: 1 wu per tick against a 0.1 wu wall whose near face is at
        // z = 3.45: segments [0,1] [1,2] [2,3] miss, [3,4] hits.
        let (per_tick, wall) = fly_at_thin_wall(60.0, 3.5, 20);
        assert_eq!(
            per_tick.len(),
            4,
            "the shot is destroyed on the fourth tick"
        );
        for (i, effects) in per_tick.iter().take(3).enumerate() {
            assert_eq!(set_position_z(effects), Some((i + 1) as f32));
        }
        let hits: usize = per_tick
            .iter()
            .flatten()
            .filter(|effect| matches!(effect, Effect::Send { msg } if msg.to == wall && matches!(msg.payload, MessagePayload::Collided { .. })))
            .count();
        assert_eq!(hits, 1, "the wall is notified exactly once");
        assert!(
            per_tick[3]
                .iter()
                .any(|effect| matches!(effect, Effect::ProjectileTrailEnd { point, .. } if (point.z - 3.5).abs() < 0.1)),
            "the trail is extended to the wall"
        );
        assert!(
            set_position_z(&per_tick[3]).is_none(),
            "a hit does not move the shot on"
        );
    }

    /// A VR shot has no ray origin, so its line starts a little behind the
    /// muzzle; the shot must not be drawn back into the gun.
    #[test]
    fn a_vr_shot_stays_at_the_muzzle_until_its_line_passes_it() {
        use crate::scripts::{Effect, Script};
        use cgmath::{Matrix4, SquareMatrix};
        let mut world = World::new();
        let projectile = world.add_entity(crate::runtime_props::RuntimePropTransform(
            Matrix4::identity(),
        ));
        let physics = PhysicsWorld::new();
        // 0.1 world units a tick.
        let mut script = super::InternalFastProjectileScript::traveling(vec3(0.0, 0.0, 1.0), 6.0);
        let tick = crate::time::Time {
            elapsed: std::time::Duration::from_secs_f32(1.0 / 60.0),
            total: std::time::Duration::ZERO,
        };
        let zs: Vec<Option<f32>> = (0..10)
            .map(|_| {
                set_position_z(&Effect::flatten(vec![
                    script.update(projectile, &world, &physics, &tick),
                ]))
            })
            .collect();
        assert!(zs.iter().flatten().all(|z| *z > 0.0), "{zs:?}");
        assert!(zs[0].is_none() && zs[9].is_some(), "{zs:?}");
    }

    #[test]
    fn a_shot_saved_mid_flight_resumes_and_hits_once() {
        use crate::scripts::{Effect, MessagePayload};
        let (straight, wall) = fly_at_thin_wall(60.0, 3.5, 20);
        let (reloaded, _) = fly_at_thin_wall_reloading(60.0, 3.5, 20, Some(2));
        let zs =
            |ticks: &[Vec<Effect>]| ticks.iter().map(|e| set_position_z(e)).collect::<Vec<_>>();
        assert_eq!(
            zs(&reloaded),
            zs(&straight),
            "same path either side of the load"
        );
        let hits = reloaded
            .iter()
            .flatten()
            .filter(|effect| matches!(effect, Effect::Send { msg } if msg.to == wall && matches!(msg.payload, MessagePayload::Collided { .. })))
            .count();
        assert_eq!(hits, 1);
    }

    #[test]
    fn a_traveling_shot_does_not_move_on_a_zero_dt_tick() {
        use crate::scripts::{Effect, Script};
        let world = World::new();
        let physics = PhysicsWorld::new();
        let mut script = super::InternalFastProjectileScript::traveling(vec3(0.0, 0.0, 1.0), 60.0);
        let effect = script.update(
            EntityId::from_inner(3).unwrap(),
            &world,
            &physics,
            &crate::time::Time::default(),
        );
        assert!(matches!(effect, Effect::NoEffect));
    }

    #[test]
    fn the_shot_mode_is_chosen_from_the_world_scale_at_launch() {
        use super::InternalFastProjectileScript;
        let velocity = vec3(0.0, 0.0, 100.0);
        assert!(!InternalFastProjectileScript::for_launch(velocity, 1.0, 1.0).is_traveling());
        let slowed = InternalFastProjectileScript::for_launch(velocity, 0.5, 0.2);
        assert!(slowed.is_traveling());
        // The launch speed, times the readability speed.
        let speed = slowed.flight.as_ref().map(|flight| flight.speed);
        assert!(
            speed.is_some_and(|speed| (speed - 20.0).abs() < 1e-4),
            "{speed:?}"
        );
    }

    #[test]
    fn grub_shots_hit_segments_beyond_support_and_miss_empty_support_volume() {
        use crate::physics::CollisionGroup;
        let mut world = World::new();
        let mut physics = PhysicsWorld::new();
        let grub = world.add_entity((
            dark::properties::PropAI("Grub".into()),
            crate::creature::RuntimePropHasHitBoxes,
        ));
        physics.add_kinematic(
            grub,
            vec3(0.0, 0.2, 1.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.4, 0.4, 0.4),
            CollisionGroup::actor(),
            false,
        );
        let segment = world.add_entity(RuntimePropHitBox {
            parent_entity_id: grub,
            hit_box_type: HitBoxType::Body,
            joint_id: 1,
        });
        physics.add_kinematic_shared_shape(
            segment,
            vec3(0.0, 0.0, 1.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            rapier3d::prelude::SharedShape::capsule_x(0.45, 0.075),
            vec3(0.0, 0.0, 0.0),
            CollisionGroup::hitbox(),
            false,
        );
        let mut player =
            physics.create_player(vec3(10.0, 10.0, 10.0), EntityId::from_inner(1000).unwrap());
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let cast = |x, y| {
            projectile_ray_cast(
                point3(x, y, 0.0),
                vec3(0.0, 0.0, 1.0),
                &physics,
                2.0,
                &world,
                false,
                None,
            )
            .and_then(|hit| hit.maybe_entity_id)
        };
        assert_eq!(
            cast(0.4, 0.0),
            Some(segment),
            "tail outside sphere is hittable"
        );
        assert_eq!(
            cast(0.0, 0.3),
            None,
            "empty upper sphere cannot absorb a shot"
        );
        assert_eq!(cast(0.4, 0.12), None, "padding must remain bounded");
    }

    fn shooter_and_target_fixture() -> (PhysicsWorld, World, EntityId, EntityId) {
        let mut physics = PhysicsWorld::new();
        // Put the player's capsule between the ray origin and the target so
        // changing only the PLAYER mask changes which entity the ray hits.
        let player_entity = EntityId::from_inner(1).unwrap();
        let mut player = physics.create_player(vec3(0.0, 0.0, 2.0), player_entity);

        let target = EntityId::from_inner(2).unwrap();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 2.0, 2.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        (physics, World::new(), player_entity, target)
    }

    #[test]
    fn projectile_raycast_hits_the_player_collider() {
        let (physics, world, player_entity, _target) = shooter_and_target_fixture();
        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            10.0,
            &world,
            true,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(player_entity),
            "an AI/turret fast projectile must be able to hit the player's dedicated collider",
        );
    }

    #[test]
    fn player_fired_projectile_raycast_shoots_past_the_shooters_collider() {
        let (physics, world, _player_entity, target) = shooter_and_target_fixture();
        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            10.0,
            &world,
            false,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(target),
            "a player-fired ray must reach the target, not the shooter",
        );
    }

    /// A turret's muzzle can sit inside its own authored collision sphere, so
    /// a solid raycast from it reports the turret at distance 0 (#1864 sized
    /// the spheres to their true radii). The launcher must be skipped.
    #[test]
    fn an_ai_shot_skips_the_launcher_whose_collider_contains_the_muzzle() {
        let mut physics = PhysicsWorld::new();
        let player_entity = EntityId::from_inner(1).unwrap();
        let mut player = physics.create_player(vec3(0.0, 0.0, 6.0), player_entity);
        let turret = EntityId::from_inner(2).unwrap();
        physics.add_kinematic(
            turret,
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 2.0, 2.0),
            crate::physics::CollisionGroup::selectable(),
            false,
        );
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        let world = World::new();
        let cast = |launcher| {
            projectile_ray_cast(
                point3(0.0, 0.0, 0.0),
                vec3(0.0, 0.0, 1.0),
                &physics,
                10.0,
                &world,
                true,
                launcher,
            )
            .and_then(|result| result.maybe_entity_id)
        };

        assert_eq!(
            cast(None),
            Some(turret),
            "fixture: the muzzle is inside the turret"
        );
        assert_eq!(
            cast(Some(turret)),
            Some(player_entity),
            "the shot must reach the player, not its own turret",
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
        let (physics, world, wall) = gap_beside_limb_fixture();

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(wall),
            "a shot that crossed the capsule but no limb should reach the wall behind",
        );
    }

    /// A human creature whose capsule spans the line of fire along +Z from
    /// the origin, its one hitbox off to the side, and a wall behind it.
    fn gap_beside_limb_fixture() -> (PhysicsWorld, World, EntityId) {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        // The creature's capsule spans the line of fire...  Creature type 0 is
        // HUMAN, whose definition maps fifteen joints, so its hitboxes stand
        // in for its body.
        let creature = world.add_entity((crate::creature::RuntimePropHasHitBoxes, PropCreature(0)));
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
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);
        (physics, world, wall)
    }

    /// A traveling shot's later segment can start inside a capsule it entered
    /// through a gap; that is not a point-blank shot, so it carries on.
    #[test]
    fn a_segment_starting_inside_a_capsule_still_passes_it_by() {
        let (physics, world, wall) = gap_beside_limb_fixture();
        let hit = super::projectile_segment_cast(
            point3(0.0, 0.0, 5.0),
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            15.0,
            &world,
            false,
            None,
        );
        assert_eq!(hit.and_then(|result| result.maybe_entity_id), Some(wall));
    }

    /// ...unless the muzzle is inside the creature. Its limbs can all lie
    /// behind the ray origin, and a shot pressed into a body must not become a
    /// terrain impact past it.
    #[test]
    fn a_point_blank_shot_still_hits_the_creature_it_is_pressed_into() {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        let creature = world.add_entity((crate::creature::RuntimePropHasHitBoxes, PropCreature(0)));
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
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(creature),
            "a muzzle inside the creature keeps its coarse hit",
        );
    }

    /// The Overlord maps a single `Body` joint, so its limbs have no proxy at
    /// all. A shot that misses that one blob has not missed the creature -
    /// passing through would leave whole bands of it unshootable - so it keeps
    /// its capsule.
    ///
    /// Negative-first: without the coverage gate this shot reaches the wall.
    #[test]
    fn a_sparse_hitbox_creature_is_still_hit_on_its_capsule() {
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();

        // Creature type 5 is OVERLORD: one mapped joint.
        let creature = world.add_entity((crate::creature::RuntimePropHasHitBoxes, PropCreature(5)));
        physics.add_kinematic(
            creature,
            vec3(0.0, 0.0, 5.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(3.0, 3.0, 1.0),
            crate::physics::CollisionGroup::actor(),
            false,
        );
        let hit_box = world.add_entity(RuntimePropHitBox {
            parent_entity_id: creature,
            hit_box_type: HitBoxType::Body,
            joint_id: 0,
        });
        // Off to the side, as a body blob is from the limbs.
        physics.add_kinematic(
            hit_box,
            vec3(1.2, 0.0, 5.0),
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
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(creature),
            "a creature whose hitboxes do not cover it keeps taking capsule hits",
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
        #[cfg(feature = "lazy-hitboxes")]
        physics.promote_hitboxes_for_test(|entity| {
            use shipyard::Get;
            world
                .borrow::<shipyard::View<RuntimePropHitBox>>()
                .unwrap()
                .get(entity)
                .map_or(entity, |hitbox| hitbox.parent_entity_id)
        });
        physics.update(vec3(0.0, 0.0, 0.0), &mut player);

        let hit = projectile_ray_cast(
            point3(0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 1.0),
            &physics,
            20.0,
            &world,
            false,
            None,
        );

        assert_eq!(
            hit.and_then(|result| result.maybe_entity_id),
            Some(corpse),
            "a body with no hitboxes is hit on its own collider, as it always was",
        );
    }

    /// A VR shot carries no camera origin, so before this marker its ray kept
    /// `PLAYER` in the mask. Held in close to the body - a natural chest or hip
    /// hold - the muzzle sits inside the player's own capsule, and a solid
    /// raycast reports the containing shape as a hit at distance 0: the shot
    /// spanged on the shooter instead of leaving the weapon, spending psi/ammo
    /// and damaging the player. What excludes the player is now this marker,
    /// set by `create_projectile` in BOTH presentations, rather than the
    /// incidental presence of a camera origin.
    ///
    /// This asserts only the predicate. The containment itself is not
    /// reproducible in a bare `PhysicsWorld` - the sibling raycast tests below
    /// pass with `can_hit_player` either way, so they never covered it - so the
    /// behavioural regression test is the earth.mis e2e
    /// (`earth-psi-cryo-self-hit.e2e.test.ts`), which fires the mission's own
    /// psi amp from a natural hold and requires the bolt to survive.
    #[test]
    fn a_projectile_from_create_projectile_is_marked_player_fired() {
        let mut world = World::new();
        let projectile = world.add_entity(RuntimePropPlayerFiredProjectile);
        assert!(is_player_fired(&world, projectile));
    }

    /// The counterpart gate: an unmarked projectile (AI, turret) still hits the
    /// player, so the fix cannot be "never hit the player".
    #[test]
    fn an_unmarked_projectile_is_not_player_fired() {
        let mut world = World::new();
        let ai_projectile = world.add_entity(());
        assert!(!is_player_fired(&world, ai_projectile));
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
