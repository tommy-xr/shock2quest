//! An object-model actor: navigation chooses a heading, physics supplies the
//! travel, and the authored joint tweq supplies the pose. No motion clips.
use super::{
    ai_util,
    joint_tweq::JointTweq,
    mobile_awareness::MobileAwareness,
    steering::{PathFollowSteeringStrategy, Steering, SteeringStrategy},
};
use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{
        AIPropertyUpdate, Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState,
        ScriptStateError,
    },
    time::Time,
};
use cgmath::{Deg, EuclideanSpace, InnerSpace, Quaternion, Rotation3, Vector3, Zero, vec3};
use dark::{
    SCALE_FACTOR,
    properties::{AIAlertLevel, PropAIGrubCombat, PropAIMoveSpeed, PropAITurnRate},
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

const KEY: &str = "shock2vr.grub_ai";
#[derive(Default, Clone, Serialize, Deserialize)]
struct GrubState {
    awareness: MobileAwareness,
    joints: JointTweq,
    leap_remaining: f32,
    leaping: bool,
    contact_spent: bool,
    dead: bool,
}

pub struct GrubAI {
    state: GrubState,
    chase: PathFollowSteeringStrategy,
    wander: PathFollowSteeringStrategy,
}
impl GrubAI {
    pub fn new() -> Self {
        Self {
            state: GrubState::default(),
            chase: PathFollowSteeringStrategy::chase_player(),
            wander: PathFollowSteeringStrategy::wander(3.0),
        }
    }
    fn die(&mut self, entity_id: EntityId) -> Effect {
        if self.state.dead {
            return Effect::NoEffect;
        }
        self.state.dead = true;
        Effect::SlayEntity { entity_id }
    }
}

/// Sample support below the actual authored sphere (not the model origin).
/// Keeping this independent of the player's controller preserves projectile
/// launches and gravity while an actor is airborne.
fn supported(physics: &PhysicsWorld, entity: EntityId, center: Vector3<f32>, radius: f32) -> bool {
    physics
        .ray_cast2_as_actor(
            cgmath::Point3::from_vec(center),
            -Vector3::unit_y(),
            radius + 0.06,
            InternalCollisionGroups::WORLD | InternalCollisionGroups::ENTITY,
            Some(entity),
            true,
        )
        .is_some_and(|hit| hit.hit_normal.y > 0.55)
}

/// Our motor has no Dark vertical velocity-control loop after the launch.
/// Treat the authored leap speeds as limits and aim the hop at the target's
/// body height. Applying the raw 50 ft/s as an uncontrolled ballistic launch
/// sends a grub several storeys above its target in an open scene.
fn leap_velocity(
    forward: Vector3<f32>,
    target_height: f32,
    radius: f32,
    horizontal: f32,
    vertical: f32,
) -> Vector3<f32> {
    let rise = (target_height + radius).max(radius * 2.0);
    let up = vertical.min((2.0 * 9.81 * rise).sqrt());
    forward * horizontal + Vector3::unit_y() * up
}

impl Script for GrubAI {
    fn script_state_key(&self) -> Option<&'static str> {
        Some(KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.state, KEY)
    }
    fn restore_state(
        &mut self,
        saved: &ScriptState,
        _: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.state = saved.decode(1, KEY)?;
        Ok(())
    }
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.state.joints = JointTweq::from_world(world, entity_id);
        self.state.awareness = MobileAwareness::from_world(world, entity_id);
        self.state.joints.pose(entity_id)
    }
    fn initialize_after_hydration(
        &mut self,
        entity_id: EntityId,
        world: &World,
        hydrated: bool,
    ) -> Effect {
        if hydrated {
            self.state.joints.pose(entity_id)
        } else {
            self.initialize(entity_id, world)
        }
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if ai_util::is_killed(entity_id, world) {
            return self.die(entity_id);
        }
        if self.state.dead {
            return Effect::NoEffect;
        }
        let dt = time.elapsed.as_secs_f32();
        if dt <= 0.0 {
            return Effect::NoEffect;
        }
        self.state.leap_remaining = (self.state.leap_remaining - dt).max(0.0);
        let (visible, awareness) = self.state.awareness.update(world, physics, entity_id, dt);
        let mut effects = vec![awareness, self.state.joints.update(entity_id, world, dt)];
        let (position, _) = ai_util::get_position_and_forward(world, entity_id);
        let velocity = physics
            .get_velocity(entity_id)
            .unwrap_or_else(Vector3::zero);
        let Some((center, radius)) = physics.actor_sphere(entity_id) else {
            return Effect::combine(effects);
        };
        let grounded = velocity.y <= 0.1
            && (physics.actor_has_support(entity_id)
                || supported(physics, entity_id, center, radius));
        // Do not overwrite any part of an emitter launch or a committed leap.
        if !grounded {
            effects.push(Effect::SetAIProperty {
                entity_id,
                update: AIPropertyUpdate::Behavior {
                    name: "Airborne".into(),
                },
            });
            return Effect::combine(effects);
        }
        self.state.leaping = false;
        let chasing = self.state.awareness.target.is_some()
            && matches!(
                self.state.awareness.alertness.current_level,
                AIAlertLevel::Moderate | AIAlertLevel::High
            );
        // The LGMD grub faces Dark +X (runtime -X); navigation headings
        // use +Z. Convert once at the movement boundary, in both directions.
        let heading = ai_util::current_yaw(entity_id, world) - Deg(90.0);
        let route = if chasing {
            &mut self.chase
        } else {
            &mut self.wander
        };
        let steering = route.steer(heading, world, physics, entity_id, time);
        // A scene without a navigation service can still exercise the actor.
        // A failed/pending real route must never become a straight-line chase.
        let no_navigation = world
            .borrow::<shipyard::UniqueView<crate::mission::GlobalPathfinding>>()
            .ok()
            .is_none_or(|service| service.0.is_none());
        let steering = steering.or_else(|| {
            (no_navigation && chasing && visible).then(|| {
                (
                    Steering::turn_to_point(
                        position,
                        cgmath::Point3::from_vec(self.state.awareness.target.unwrap()),
                    ),
                    Effect::NoEffect,
                )
            })
        });
        let mut desired_velocity = vec3(0.0, velocity.y.min(0.0), 0.0);
        if let Some((steering, effect)) = steering {
            effects.push(effect);
            let delta = ai_util::clamp_to_minimal_delta_angle(steering.desired_heading - heading).0;
            let rate = world
                .borrow::<View<PropAITurnRate>>()
                .unwrap()
                .get(entity_id)
                .map(|p| p.0)
                .unwrap_or(380.0);
            let facing = heading + Deg(delta.clamp(-rate * dt, rate * dt));
            let rotation = Quaternion::from_angle_y(facing);
            let forward = rotation * Vector3::unit_z();
            let speed = world
                .borrow::<View<PropAIMoveSpeed>>()
                .unwrap()
                .get(entity_id)
                .map(|p| p.0)
                .unwrap_or(1.4 / SCALE_FACTOR)
                * 7.5; // Dark SetObjImpulse's move-speed multiplier.
            // Crawl only into supported ground. Airborne leaps are a separate
            // deliberate action; ground navigation must not walk off an edge.
            if supported(
                physics,
                entity_id,
                center + forward * radius.max(speed * dt),
                radius,
            ) {
                desired_velocity += forward * speed * delta.to_radians().cos().max(0.0);
            }
            effects.push(Effect::SetRotation {
                entity_id,
                rotation: Quaternion::from_angle_y(facing + Deg(90.0)),
            });
            if chasing && visible && delta.abs() < 30.0 && self.state.leap_remaining <= 0.0 {
                if let Ok(config) = world
                    .borrow::<View<PropAIGrubCombat>>()
                    .unwrap()
                    .get(entity_id)
                {
                    let distance =
                        (self.state.awareness.target.unwrap() - position.to_vec()).magnitude();
                    if distance <= config.leap_distance && config.vertical_speed > 0.0 {
                        desired_velocity = leap_velocity(
                            forward,
                            self.state.awareness.target.unwrap().y - center.y,
                            radius,
                            config.horizontal_speed,
                            config.vertical_speed,
                        );
                        self.state.leaping = true;
                        self.state.contact_spent = false;
                        // Retain the selected delay in the script snapshot. It
                        // must not re-roll or re-launch when a save is loaded.
                        use rand::Rng;
                        self.state.leap_remaining = rand::thread_rng().gen_range(
                            config.min_leap_ms..=config.max_leap_ms.max(config.min_leap_ms),
                        ) as f32
                            / 1000.0;
                    }
                }
            }
        }
        effects.push(Effect::SetLinearVelocity {
            entity_id,
            velocity: desired_velocity,
        });
        effects.push(Effect::SetAIProperty {
            entity_id,
            update: AIPropertyUpdate::Behavior {
                name: if self.state.leaping {
                    "Leap"
                } else if chasing {
                    "Chase"
                } else {
                    "Wander"
                }
                .into(),
            },
        });
        Effect::combine(effects)
    }
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Slay => self.die(entity_id),
            MessagePayload::SetAlertness { level, pin } => self
                .state
                .awareness
                .set_alertness(world, entity_id, *level, *pin),
            MessagePayload::HeardNoise { origin } => {
                self.state.awareness.hear(world, entity_id, *origin)
            }
            MessagePayload::Collided { with, .. }
                if !self.state.dead && !self.state.contact_spent =>
            {
                let effect = crate::scripts::script_util::projectile_contact_effects(
                    world, entity_id, *with, None,
                );
                if !matches!(effect, Effect::NoEffect) {
                    self.state.contact_spent = true;
                }
                effect
            }
            _ => Effect::NoEffect,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hop_uses_authored_speed_limits_without_ballistic_overshoot() {
        let v = leap_velocity(Vector3::unit_z(), 1.0, 0.2, 4.8, 20.0);
        assert!((v.y * v.y / (2.0 * 9.81) - 1.2).abs() < 0.0001);
        assert_eq!(v.z, 4.8);
        assert_eq!(leap_velocity(Vector3::unit_z(), 1.0, 0.2, 2.0, 1.0).y, 1.0);
    }
    #[test]
    fn a_saved_committed_leap_and_contact_latch_are_not_restarted() {
        let mut grub = GrubAI::new();
        grub.state.leaping = true;
        grub.state.contact_spent = true;
        grub.state.leap_remaining = 0.75;
        grub.state.awareness.target = Some(vec3(1.0, 2.0, 3.0));
        let saved = grub.save_state().unwrap();
        let map = std::collections::HashMap::new();
        let mut restored = GrubAI::new();
        restored
            .restore_state(
                &saved,
                &ScriptRestoreContext {
                    entity_id_map: &map,
                },
            )
            .unwrap();
        assert_eq!(restored.save_state().unwrap(), saved);
        assert!(restored.state.leaping && restored.state.contact_spent);
    }
}
