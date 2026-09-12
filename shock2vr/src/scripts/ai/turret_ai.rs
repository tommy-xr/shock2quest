use cgmath::{Deg, Matrix4, Quaternion, Rotation3, SquareMatrix, Transform, point3};
use dark::properties::{
    AIAlertLevel, PropAIAlertCap, PropAIAwareDelay, PropAIDevice, PropAITurnRate,
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

use super::{
    Effect, MessagePayload, Script,
    ai_debug_util::{self, AlertnessDebugConfig, FovDebugConfig},
    ai_util,
    alertness::{self, AlertnessState, AlertnessTimings},
};
use crate::{
    physics::PhysicsWorld,
    runtime_props::{RuntimePropObjectArticulation, RuntimePropTransform},
    time::Time,
};

const DEFAULT_ESCALATE_SECONDS: f32 = 2.0;
const DEFAULT_DECAY_SECONDS: f32 = 4.0;
// Dark aiprcore.h: AIGetTurnRate's default, in degrees/second.
const DEFAULT_TURN_RATE: f32 = 380.0;
// cAIJointSlideAction::Enact increments by activateSpeed each AI frame, ignoring
// deltaTime. Normalize that legacy increment to our fixed 60 Hz simulation so
// desktop/VR frame rates do not change the opening duration.
const LEGACY_SLIDE_HZ: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum TurretState {
    Closed,
    Opening,
    Open,
    Returning,
    Closing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TurretPose {
    state: TurretState,
    activation: f32,
    facing: f32,
    fire_cooldown: f32,
}

/// Convert a world target into the mounted model's facing parameter. Doing this
/// in model space also handles a rotated mount without subtracting Euler yaws.
fn target_parameter(root: Matrix4<f32>, target: cgmath::Point3<f32>) -> Option<f32> {
    let local = root.invert()?.transform_point(target);
    if local.x * local.x + local.z * local.z < 1e-8 {
        return None;
    }
    Some(angle_delta(local.x.atan2(local.z).to_degrees() + 90.0, 0.0))
}

fn angle_delta(target: f32, current: f32) -> f32 {
    (target - current + 180.0).rem_euclid(360.0) - 180.0
}
fn approach(current: f32, target: f32, step: f32) -> f32 {
    current + (target - current).clamp(-step, step)
}
fn turn(current: f32, target: f32, step: f32) -> f32 {
    angle_delta(
        current + angle_delta(target, current).clamp(-step, step),
        0.0,
    )
}

impl TurretPose {
    fn new(inactive: f32) -> Self {
        Self {
            state: TurretState::Closed,
            activation: inactive,
            facing: 0.0,
            fire_cooldown: 0.0,
        }
    }
    fn advance(
        &mut self,
        visible: bool,
        target: f32,
        device: &PropAIDevice,
        turn_rate: f32,
        dt: f32,
    ) -> Option<&'static str> {
        let dt = dt.max(0.0);
        self.fire_cooldown = (self.fire_cooldown - dt).max(0.0);
        let mut sound = None;
        if visible
            && matches!(
                self.state,
                TurretState::Closed | TurretState::Returning | TurretState::Closing
            )
        {
            self.state = TurretState::Opening;
            sound = Some("activate");
        } else if !visible && matches!(self.state, TurretState::Open | TurretState::Opening) {
            self.state = TurretState::Returning;
        }
        let activation_step = if device.activate_rotate {
            turn_rate
        } else {
            device.activate_speed.abs() * LEGACY_SLIDE_HZ
        } * dt;
        match self.state {
            TurretState::Opening => {
                self.activation = approach(self.activation, device.active_pos, activation_step);
                if self.activation == device.active_pos {
                    self.state = TurretState::Open;
                }
            }
            TurretState::Open => self.facing = turn(self.facing, target, turn_rate * dt),
            TurretState::Returning => {
                self.facing = turn(self.facing, 0.0, turn_rate * dt);
                if self.facing == 0.0 {
                    self.state = TurretState::Closing;
                    sound = Some("deactivate");
                }
            }
            TurretState::Closing => {
                self.activation = approach(self.activation, device.inactive_pos, activation_step);
                if self.activation == device.inactive_pos {
                    self.state = TurretState::Closed;
                }
            }
            TurretState::Closed => {}
        }
        sound
    }
    fn parameters(&self, device: &PropAIDevice) -> Vec<(i32, f32)> {
        vec![
            (device.joint_activate, self.activation),
            (device.joint_rotate, self.facing),
        ]
    }
    fn can_fire(&self, visible: bool, target: f32, epsilon: f32) -> bool {
        visible
            && self.state == TurretState::Open
            && self.fire_cooldown == 0.0
            && angle_delta(target, self.facing).abs() <= epsilon.to_degrees()
    }
}

#[derive(Clone)]
struct TurretConfig {
    alert_cap: PropAIAlertCap,
    timings: AlertnessTimings,
}

pub struct TurretAI {
    pose: TurretPose,
    restored: bool,
    alertness: AlertnessState,
    config: Option<TurretConfig>,
    device: Option<PropAIDevice>,
    turn_rate: f32,
}

impl TurretAI {
    pub fn new() -> Self {
        Self {
            pose: TurretPose::new(0.0),
            restored: false,
            alertness: AlertnessState::default(),
            config: None,
            device: None,
            turn_rate: DEFAULT_TURN_RATE,
        }
    }
    fn build_config(world: &World, entity_id: EntityId) -> Option<TurretConfig> {
        let (v_alert_cap, v_aware_delay): (View<PropAIAlertCap>, View<PropAIAwareDelay>) =
            world.borrow().ok()?;

        let alert_cap = v_alert_cap
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(PropAIAlertCap {
                max_level: AIAlertLevel::High,
                min_level: AIAlertLevel::Lowest,
                min_relax: AIAlertLevel::Low,
            });

        // Build default aware delay for turrets
        let default_aware_delay = PropAIAwareDelay {
            to_two: (DEFAULT_ESCALATE_SECONDS * 1000.0) as i32,
            to_three: (DEFAULT_ESCALATE_SECONDS * 1000.0) as i32,
            two_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
            three_reuse: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
            ignore_range: (DEFAULT_DECAY_SECONDS * 1000.0) as i32,
        };

        let aware_delay = v_aware_delay
            .get(entity_id)
            .ok()
            .cloned()
            .unwrap_or(default_aware_delay);

        let timings = AlertnessTimings::from_aware_delay(&aware_delay);

        Some(TurretConfig { alert_cap, timings })
    }
}

impl Script for TurretAI {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.config = Self::build_config(world, entity_id);
        if !self.restored {
            if let Some(config) = &self.config {
                self.alertness = AlertnessState::new(alertness::clamp_level(
                    AIAlertLevel::Lowest,
                    &config.alert_cap,
                ));
            }
        }
        self.device = world
            .borrow::<View<PropAIDevice>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().cloned());
        self.turn_rate = world
            .borrow::<View<PropAITurnRate>>()
            .ok()
            .and_then(|v| v.get(entity_id).ok().map(|p| p.0))
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(DEFAULT_TURN_RATE);
        if let Some(device) = &self.device {
            if !self.restored {
                self.pose = TurretPose::new(device.inactive_pos);
            }
            return Effect::combine(vec![
                alertness::sync_alertness_effect(entity_id, &self.alertness),
                Effect::SetObjectParameters {
                    entity_id,
                    parameters: self.pose.parameters(device),
                },
            ]);
        }
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let Some(device) = &self.device else {
            return Effect::NoEffect;
        };
        let dt = time.elapsed.as_secs_f32();
        // Object models face Dark +X, converted to model -X. The shared FOV
        // helper uses +Z and negates its heading argument; convert only here.
        // This is a coordinate-system boundary, not a correction to a bone.
        let heading = Deg(90.0 - self.pose.facing);
        let visible = ai_util::is_player_visible_in_fov(entity_id, world, physics, heading, 30.0);
        let target = ai_util::chase_target(world, entity_id)
            .and_then(|target| {
                let transforms = world.borrow::<View<RuntimePropTransform>>().ok()?;
                let root = transforms.get(entity_id).ok()?.0;
                target_parameter(root, point3(target.x, target.y, target.z))
            })
            .unwrap_or(self.pose.facing);
        let mut effects = Vec::new();
        if let Some(config) = &self.config {
            if alertness::process_alertness_update(
                &mut self.alertness,
                visible,
                dt,
                &config.timings,
                &config.alert_cap,
            )
            .is_some()
            {
                effects.push(alertness::sync_alertness_effect(entity_id, &self.alertness));
            }
        }
        if let Some(event) = self
            .pose
            .advance(visible, target, device, self.turn_rate, dt)
        {
            effects.push(ai_util::play_positional_sound(
                entity_id,
                world,
                None,
                vec![("event", event)],
            ));
        }
        let parameters = self.pose.parameters(device);
        effects.push(Effect::SetObjectParameters {
            entity_id,
            parameters: parameters.clone(),
        });
        if self.pose.can_fire(visible, target, device.facing_epsilon) {
            let rig = world
                .borrow::<View<RuntimePropObjectArticulation>>()
                .unwrap();
            let transforms = world.borrow::<View<RuntimePropTransform>>().unwrap();
            if let (Ok(rig), Ok(root)) = (rig.get(entity_id), transforms.get(entity_id)) {
                if let Some(muzzle) = rig.0.vhot_position(0, &parameters) {
                    // Resolve from this frame's parameters, not the previous
                    // frame's ECS palette: effects apply after scripts return.
                    let position = root.0.transform_point(muzzle);
                    let orientation = root.0
                        * Matrix4::from(Quaternion::from_angle_y(Deg(self.pose.facing - 90.0)));
                    let mut muzzle_transform = orientation;
                    muzzle_transform.w = position.to_homogeneous();
                    effects.push(ai_util::fire_ranged_weapon(
                        world,
                        entity_id,
                        muzzle_transform,
                    ));
                    self.pose.fire_cooldown = 1.0;
                }
            }
        }
        effects.push(ai_debug_util::draw_debug_alertness(
            world,
            entity_id,
            &self.alertness,
            visible,
            &AlertnessDebugConfig::turret(),
        ));
        effects.push(ai_debug_util::draw_debug_fov(
            world,
            entity_id,
            Deg(90.0 - self.pose.facing),
            visible,
            &FovDebugConfig::turret(),
        ));
        Effect::combine(effects)
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some("shock2vr.turret")
    }
    fn save_state(&self) -> Result<super::super::ScriptState, super::super::ScriptStateError> {
        super::super::ScriptState::encode(1, &(&self.pose, &self.alertness), "shock2vr.turret")
    }
    fn restore_state(
        &mut self,
        state: &super::super::ScriptState,
        _context: &super::super::ScriptRestoreContext<'_>,
    ) -> Result<(), super::super::ScriptStateError> {
        (self.pose, self.alertness) = state.decode(1, "shock2vr.turret")?;
        self.restored = true;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::{BaseMonster, ScriptRestoreContext};
    use std::collections::HashMap;

    fn device() -> PropAIDevice {
        PropAIDevice {
            joint_activate: 0,
            inactive_pos: 0.0,
            active_pos: 2.0,
            activate_speed: 0.1,
            joint_rotate: 1,
            facing_epsilon: 0.04,
            activate_rotate: false,
        }
    }

    #[test]
    fn target_facing_is_relative_to_the_mounted_models_authored_forward() {
        for yaw in [0.0, 90.0, 180.0, -35.0] {
            let root = Matrix4::from_translation(cgmath::vec3(4.0, 2.0, 12.0))
                * Matrix4::from_angle_y(Deg(yaw));
            for parameter in [0.0, 20.0, -25.0, 179.0] {
                let local_target =
                    Matrix4::from_angle_y(Deg(parameter)).transform_point(point3(-10.0, 0.0, 0.0));
                let actual = target_parameter(root, root.transform_point(local_target)).unwrap();
                assert!(angle_delta(parameter, actual).abs() < 0.001);
            }
        }
    }

    #[test]
    fn authored_travel_is_frame_rate_independent_and_clamps_at_both_ends() {
        for hz in [30, 60, 90, 120] {
            let mut pose = TurretPose::new(0.0);
            for _ in 0..hz {
                pose.advance(true, 0.0, &device(), 90.0, 1.0 / hz as f32);
            }
            assert_eq!(pose.activation, 2.0);
            assert_eq!(pose.state, TurretState::Open);
            for _ in 0..hz {
                pose.advance(false, 0.0, &device(), 90.0, 1.0 / hz as f32);
            }
            assert_eq!(pose.activation, 0.0);
            assert_eq!(pose.state, TurretState::Closed);
        }
        let mut pose = TurretPose::new(0.0);
        pose.advance(true, 0.0, &device(), 90.0, 0.1);
        assert!((pose.activation - 0.6).abs() < 1e-6);
        assert!(!pose.can_fire(true, 0.0, 0.04));
    }

    #[test]
    fn turn_before_firing_and_return_home_before_lowering() {
        let mut pose = TurretPose::new(0.0);
        pose.advance(true, 30.0, &device(), 90.0, 1.0);
        assert!(!pose.can_fire(true, 30.0, 0.04));
        pose.advance(true, 30.0, &device(), 90.0, 0.1);
        assert_eq!(pose.facing, 9.0);
        pose.advance(true, 30.0, &device(), 90.0, 1.0);
        assert!(pose.can_fire(true, 30.0, 0.04));
        pose.advance(false, 30.0, &device(), 90.0, 0.1);
        assert_eq!(pose.state, TurretState::Returning);
        assert_eq!(pose.activation, 2.0);
        assert_eq!(pose.facing, 21.0);
        pose.advance(false, 30.0, &device(), 90.0, 1.0);
        assert_eq!(pose.state, TurretState::Closing);
        assert_eq!(pose.facing, 0.0);
        assert_eq!(pose.activation, 2.0);
        pose.advance(false, 30.0, &device(), 90.0, 1.0);
        assert_eq!(pose.state, TurretState::Closed);
    }

    #[test]
    fn reacquisition_reverses_from_current_height_and_turn_takes_shortest_arc() {
        assert_eq!(turn(179.0, -179.0, 1.0), -180.0);
        let mut pose = TurretPose::new(0.0);
        pose.advance(true, 0.0, &device(), 90.0, 1.0);
        pose.advance(false, 0.0, &device(), 90.0, 0.1);
        pose.advance(false, 0.0, &device(), 90.0, 0.1);
        assert!((pose.activation - 1.4).abs() < 1e-5);
        pose.advance(true, 0.0, &device(), 90.0, 0.05);
        assert!((pose.activation - 1.7).abs() < 1e-5);
        assert_eq!(pose.state, TurretState::Opening);
    }

    #[test]
    fn native_turret_pose_round_trips_through_base_monster() {
        let mut turret = TurretAI::new();
        turret.pose.activation = 1.1;
        turret.pose.facing = 27.0;
        turret.pose.state = TurretState::Returning;
        turret.pose.fire_cooldown = 0.4;
        turret.alertness.current_level = AIAlertLevel::High;
        turret.alertness.hidden_time = 1.25;
        let saved = turret.save_state().unwrap();
        let outer = super::super::super::ScriptState::encode(
            2,
            &serde_json::json!({
                "stasis": null, "turret": saved
            }),
            "shock2vr.ai_stasis",
        )
        .unwrap();
        let mut base = BaseMonster::new();
        base.restore_state(&outer, &ScriptRestoreContext::new(&HashMap::new()))
            .unwrap();
        let mut world = World::new();
        let entity = world.add_entity((
            dark::properties::PropAI("turret".into()),
            device(),
            RuntimePropTransform(Matrix4::identity()),
            dark::properties::PropPosition {
                position: cgmath::vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: Quaternion::from_angle_y(Deg(0.0)),
            },
        ));
        base.initialize_after_hydration(entity, &world, true);
        assert_eq!(base.save_state().unwrap(), outer);
    }
}
