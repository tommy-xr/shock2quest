//! Particle-cloud AI. Ground navigation supplies horizontal routes; a separate
//! motor follows the authored hover height and sweeps the cloud's solid core.
use super::{
    ai_util,
    mobile_awareness::MobileAwareness,
    steering::{PathFollowSteeringStrategy, SteeringStrategy},
};
use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::{
        AIPropertyUpdate, Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState,
        ScriptStateError,
    },
    time::Time,
};
use cgmath::{Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation3, Vector3, Zero, vec3};
use dark::{
    SCALE_FACTOR,
    properties::{
        AIAlertLevel, Link, PropAIMoveSpeed, PropAIMoveZOffset, PropAISwarm, StimPropagator,
    },
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};

/// Retail authors a point collider. A small finite core keeps the cloud
/// shootable and gives Rapier a stable body without making every bug solid.
pub const SWARM_CORE_RADIUS: f32 = 0.3;
const KEY: &str = "shock2vr.swarmer_ai";
const PULSE_SECONDS: f32 = 0.5;

#[derive(Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
enum Mode {
    #[default]
    Close,
    BackOff,
}
#[derive(Default, Clone, Serialize, Deserialize)]
struct SwarmerState {
    awareness: MobileAwareness,
    mode: Mode,
    retreat: Option<Vector3<f32>>,
    retry_remaining: f32,
    bob_phase: f32,
    pulse_remaining: f32,
    dead: bool,
}
pub struct SwarmerAI {
    state: SwarmerState,
    chase: PathFollowSteeringStrategy,
    wander: PathFollowSteeringStrategy,
    retreat_route: Option<PathFollowSteeringStrategy>,
}
impl SwarmerAI {
    pub fn new() -> Self {
        Self {
            state: SwarmerState::default(),
            chase: PathFollowSteeringStrategy::chase_player(),
            wander: PathFollowSteeringStrategy::wander(3.0),
            retreat_route: None,
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

fn floor_height(physics: &PhysicsWorld, entity: EntityId, position: Vector3<f32>) -> Option<f32> {
    physics
        .ray_cast2_as_actor(
            Point3::from_vec(position),
            -Vector3::unit_y(),
            12.0,
            InternalCollisionGroups::WORLD,
            Some(entity),
            true,
        )
        .filter(|hit| hit.hit_normal.y > 0.55)
        .map(|hit| hit.hit_point.y)
}

/// Original cAISwarmer adds half a Dark foot on a four-second sine wave.
fn hover_height(base: f32, phase: f32) -> f32 {
    base + (phase * std::f32::consts::TAU / 4.0).sin() * (0.5 / SCALE_FACTOR)
}

fn clearance(
    physics: &PhysicsWorld,
    entity: EntityId,
    position: Vector3<f32>,
    direction: Vector3<f32>,
    distance: f32,
) -> f32 {
    physics.projectile_spawn_distance(
        Point3::from_vec(position),
        direction,
        distance,
        SWARM_CORE_RADIUS,
        &|other| other != entity,
    )
}

/// Probe the intended flight volume, including upward/downward motion. If a
/// wall blocks it, choose a nearby heading that still advances toward the goal.
fn flight_velocity(
    physics: &PhysicsWorld,
    entity: EntityId,
    position: Vector3<f32>,
    desired: Vector3<f32>,
    dt: f32,
) -> Vector3<f32> {
    let speed = desired.magnitude();
    if speed < 0.001 {
        return Vector3::zero();
    }
    let probe = (speed * 0.25).max(SWARM_CORE_RADIUS * 2.0);
    let direction = desired / speed;
    let mut best = Vector3::zero();
    let mut best_score = 0.0;
    for angle in [0.0, 35.0, -35.0, 70.0, -70.0, 90.0, -90.0] {
        let candidate = Quaternion::from_angle_y(Deg(angle)) * direction;
        let room = (clearance(physics, entity, position, candidate, probe) - 0.02).max(0.0);
        let allowed_speed = speed.min(room / dt.max(0.001)).min(speed * room / probe);
        let score = allowed_speed * candidate.dot(direction).max(0.05);
        if score > best_score {
            best_score = score;
            best = candidate * allowed_speed;
        }
    }
    best
}

fn backoff_point(
    world: &World,
    physics: &PhysicsWorld,
    entity: EntityId,
    position: Vector3<f32>,
    distance: f32,
) -> Option<Vector3<f32>> {
    use rand::Rng;
    let first = rand::thread_rng().gen_range(0.0..360.0);
    for i in 0..8 {
        let direction = Quaternion::from_angle_y(Deg(first + i as f32 * 45.0)) * Vector3::unit_z();
        if clearance(physics, entity, position, direction, distance) < distance - 0.05 {
            continue;
        }
        let target = position + direction * distance;
        if floor_height(physics, entity, target).is_none() {
            continue;
        }
        let on_nav = world
            .borrow::<UniqueView<crate::mission::GlobalPathfinding>>()
            .ok()
            .is_none_or(|nav| {
                nav.0
                    .as_ref()
                    .is_none_or(|nav| nav.cell_from_position(target).is_some())
            });
        if on_nav {
            return Some(target);
        }
    }
    None
}

impl Script for SwarmerAI {
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
        self.retreat_route = self.state.retreat.map(PathFollowSteeringStrategy::to_point);
        Ok(())
    }
    fn initialize(&mut self, entity: EntityId, world: &World) -> Effect {
        self.state.awareness = MobileAwareness::from_world(world, entity);
        Effect::NoEffect
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
        self.state.bob_phase = (self.state.bob_phase + dt) % 4.0;
        self.state.retry_remaining = (self.state.retry_remaining - dt).max(0.0);
        self.state.pulse_remaining -= dt;
        let (visible, awareness) = self.state.awareness.update(world, physics, entity_id, dt);
        let mut effects = vec![awareness];
        let (pos, _) = ai_util::get_position_and_forward(world, entity_id);
        let position = pos.to_vec();
        let chasing = self.state.awareness.target.is_some()
            && matches!(
                self.state.awareness.alertness.current_level,
                AIAlertLevel::Moderate | AIAlertLevel::High
            );
        let config = world
            .borrow::<View<PropAISwarm>>()
            .unwrap()
            .get(entity_id)
            .cloned()
            .unwrap_or_default();
        let close_distance = config.close_distance.max(SWARM_CORE_RADIUS + 0.7);
        let target = self.state.awareness.target.unwrap_or(position);
        let distance = (target - position).magnitude();
        if chasing
            && self.state.mode == Mode::Close
            && visible
            && distance < close_distance
            && self.state.retry_remaining <= 0.0
        {
            if let Some(retreat) = backoff_point(
                world,
                physics,
                entity_id,
                position,
                config.backoff_distance.max(close_distance),
            ) {
                self.state.mode = Mode::BackOff;
                self.state.retreat = Some(retreat);
                self.retreat_route = Some(PathFollowSteeringStrategy::to_point(retreat));
            }
            self.state.retry_remaining = 4.0;
        } else if self.state.mode == Mode::BackOff
            && (!chasing
                || self.state.retry_remaining <= 0.0
                || self
                    .state
                    .retreat
                    .is_some_and(|p| (p - position).magnitude() < 0.5))
        {
            self.state.mode = Mode::Close;
            self.state.retreat = None;
            self.retreat_route = None;
            self.state.retry_remaining = 0.0;
        }
        let heading = ai_util::current_yaw(entity_id, world);
        let route = if chasing {
            if self.state.mode == Mode::BackOff {
                self.retreat_route.as_mut().unwrap()
            } else {
                &mut self.chase
            }
        } else {
            &mut self.wander
        };
        let steering = route.steer(heading, world, physics, entity_id, time);
        let no_nav = world
            .borrow::<UniqueView<crate::mission::GlobalPathfinding>>()
            .ok()
            .is_none_or(|n| n.0.is_none());
        let direction = if let Some((steering, effect)) = steering {
            effects.push(effect);
            Some(Quaternion::from_angle_y(steering.desired_heading) * Vector3::unit_z())
        } else if no_nav && chasing && visible {
            let delta = self.state.retreat.unwrap_or(target) - position;
            let flat = vec3(delta.x, 0.0, delta.z);
            (flat.magnitude2() > 0.001).then(|| flat.normalize())
        } else {
            None
        };
        let hover = world
            .borrow::<View<PropAIMoveZOffset>>()
            .unwrap()
            .get(entity_id)
            .map(|p| p.0)
            .unwrap_or(3.0 / SCALE_FACTOR);
        let floor = floor_height(physics, entity_id, position);
        let desired_y = floor
            .map(|floor| {
                floor + hover_height(hover.max(SWARM_CORE_RADIUS * 2.0), self.state.bob_phase)
            })
            .unwrap_or(position.y);
        let speed = world
            .borrow::<View<PropAIMoveSpeed>>()
            .unwrap()
            .get(entity_id)
            .map(|p| p.0)
            .unwrap_or(1.0)
            * 7.5;
        let horizontal_speed = if self.state.mode == Mode::Close && chasing {
            speed.min(distance * 3.0)
        } else {
            speed
        };
        let desired = direction.unwrap_or_else(Vector3::zero) * horizontal_speed
            + Vector3::unit_y() * ((desired_y - position.y) * 4.0).clamp(-3.0, 3.0);
        let velocity = flight_velocity(physics, entity_id, position, desired, dt);
        effects.push(Effect::SetLinearVelocity {
            entity_id,
            velocity,
        });
        if velocity.x * velocity.x + velocity.z * velocity.z > 0.01 {
            effects.push(Effect::SetRotation {
                entity_id,
                rotation: Quaternion::from_angle_y(Deg(velocity.x.atan2(velocity.z).to_degrees())),
            });
        }
        effects.push(Effect::SetAIProperty {
            entity_id,
            update: AIPropertyUpdate::Behavior {
                name: if !chasing {
                    "Hover/Wander"
                } else if self.state.mode == Mode::BackOff {
                    "BackOff"
                } else {
                    "Close"
                }
                .into(),
            },
        });
        if self.state.pulse_remaining <= 0.0 {
            self.state.pulse_remaining = PULSE_SECONDS;
            for (stim_template_id, options) in
                crate::scripts::script_util::get_all_links_with_template(world, entity_id, |link| {
                    match link {
                        Link::StimSource(options) => Some(*options),
                        _ => None,
                    }
                })
            {
                if let StimPropagator::Radius { radius } = options.propagator {
                    effects.push(Effect::RadiusStim {
                        source_entity_id: Some(entity_id),
                        center: position,
                        radius,
                        intensity: options.intensity,
                        stim_template_id,
                    });
                }
            }
        }
        Effect::combine(effects)
    }
    fn handle_message(
        &mut self,
        entity: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Slay => self.die(entity),
            MessagePayload::SetAlertness { level, pin } => self
                .state
                .awareness
                .set_alertness(world, entity, *level, *pin),
            MessagePayload::HeardNoise { origin } => {
                self.state.awareness.hear(world, entity, *origin)
            }
            _ => Effect::NoEffect,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flight_core_does_not_cross_a_wall_or_ceiling() {
        use crate::physics::CollisionGroup;
        let mut physics = PhysicsWorld::new();
        let mut world = World::new();
        let wall = world.add_entity(());
        let entity = world.add_entity(());
        let rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        physics.add_kinematic(
            wall,
            vec3(0.0, 0.0, 1.0),
            rotation,
            Vector3::zero(),
            vec3(10.0, 10.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            physics.create_player(vec3(20.0, 20.0, 20.0), EntityId::from_inner(1000).unwrap());
        physics.update(Vector3::zero(), &mut player);
        let start = vec3(0.0, 0.0, 0.55);
        let velocity = flight_velocity(&physics, entity, start, vec3(0.0, 0.0, 7.5), 1.0 / 60.0);
        assert!((start + velocity / 60.0).z + SWARM_CORE_RADIUS < 0.9);
        let ceiling = world.add_entity(());
        physics.add_kinematic(
            ceiling,
            vec3(0.0, 1.0, 0.0),
            rotation,
            Vector3::zero(),
            vec3(10.0, 0.2, 10.0),
            CollisionGroup::entity(),
            false,
        );
        physics.update(Vector3::zero(), &mut player);
        let start = vec3(0.0, 0.55, 0.0);
        let velocity = flight_velocity(&physics, entity, start, vec3(0.0, 3.0, 0.0), 1.0 / 60.0);
        assert!((start + velocity / 60.0).y + SWARM_CORE_RADIUS < 0.9);
    }

    #[test]
    fn authored_hover_cycle_is_four_seconds_and_half_a_dark_foot() {
        assert!((hover_height(1.2, 1.0) - 1.4).abs() < 1e-6);
        assert!((hover_height(1.2, 3.0) - 1.0).abs() < 1e-6);
        assert!((hover_height(1.2, 4.0) - 1.2).abs() < 1e-6);
    }
    #[test]
    fn retreat_and_pulse_phase_survive_hydration() {
        let mut ai = SwarmerAI::new();
        ai.state.mode = Mode::BackOff;
        ai.state.retreat = Some(vec3(1.0, 2.0, 3.0));
        ai.state.retry_remaining = 2.25;
        ai.state.bob_phase = 1.5;
        ai.state.pulse_remaining = 0.3;
        let saved = ai.save_state().unwrap();
        let mut restored = SwarmerAI::new();
        restored
            .restore_state(
                &saved,
                &ScriptRestoreContext::new(&std::collections::HashMap::new()),
            )
            .unwrap();
        assert_eq!(saved, restored.save_state().unwrap());
        assert!(restored.retreat_route.is_some());
    }
}
