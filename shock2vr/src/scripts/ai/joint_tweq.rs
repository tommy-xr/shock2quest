//! Articulated-object animation independent of AI decisions or skeletal clips.
//! The caller owns this state so animation phase freezes and saves with it.
use crate::scripts::Effect;
use dark::properties::{
    PropJointPositions, PropTweqJointsConfig, PropTweqJointsState, TweqAnimationConfig as Config,
    TweqAnimationState as State, TweqHalt,
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct JointTweq {
    values: [f32; 6],
    state: Option<PropTweqJointsState>,
}
impl JointTweq {
    pub fn from_world(world: &World, entity: EntityId) -> Self {
        Self {
            values: world
                .borrow::<View<PropJointPositions>>()
                .unwrap()
                .get(entity)
                .map(|v| v.0)
                .unwrap_or([0.0; 6]),
            state: world
                .borrow::<View<PropTweqJointsState>>()
                .unwrap()
                .get(entity)
                .ok()
                .cloned(),
        }
    }
    pub fn pose(&self, entity_id: EntityId) -> Effect {
        Effect::SetObjectParameters {
            entity_id,
            parameters: self
                .values
                .iter()
                .enumerate()
                .map(|(i, &value)| (i as i32, value))
                .collect(),
        }
    }
    pub fn update(&mut self, entity_id: EntityId, world: &World, seconds: f32) -> Effect {
        let configs = world.borrow::<View<PropTweqJointsConfig>>().unwrap();
        let (Some(state), Ok(config)) = (self.state.as_mut(), configs.get(entity_id)) else {
            return Effect::NoEffect;
        };
        if !state.animation_state.contains(State::ON) || seconds <= 0.0 {
            return self.pose(entity_id);
        }
        let primary = config
            .primary_joint
            .checked_sub(1)
            .map(usize::from)
            .filter(|i| *i < 6);
        if let Some(i) = primary {
            state.joints[i] = state.animation_state;
        }
        for (i, joint) in config.joints.iter().enumerate() {
            if !state.joints[i].contains(State::ON) || joint.limits.rate == 0.0 {
                continue;
            }
            // Grub authors linear curves. Jitter/multiply need their own
            // reproducible curve implementation, not an invented sine wave.
            if joint.curve != 0 {
                continue;
            }
            let reverse = state.joints[i].contains(State::REVERSE);
            let rate = joint.limits.rate * if reverse { -1.0 } else { 1.0 };
            self.values[i] += rate * seconds * 10.0;
            if joint.animation_config.contains(Config::NOLIMT) {
                continue;
            }
            let low = self.values[i] < joint.limits.low;
            let high = self.values[i] > joint.limits.high;
            if !low && !high {
                continue;
            }
            if joint.animation_config.contains(Config::WRAP) {
                self.values[i] = if low {
                    joint.limits.high
                } else {
                    joint.limits.low
                };
            } else {
                self.values[i] = self.values[i].clamp(joint.limits.low, joint.limits.high);
                state.joints[i].toggle(State::REVERSE);
            }
            let completed = !joint.animation_config.contains(Config::ONEBOUNCE) || reverse;
            if completed && !matches!(config.halt, TweqHalt::Continue) {
                state.joints[i].remove(State::ON);
                if primary == Some(i) {
                    state.animation_state = state.joints[i];
                    match config.halt {
                        TweqHalt::DestroyObject => return Effect::DestroyEntity { entity_id },
                        TweqHalt::SlayObj => return Effect::SlayEntity { entity_id },
                        _ => {}
                    }
                }
            }
        }
        if let Some(i) = primary {
            state.animation_state = state.joints[i];
        }
        self.pose(entity_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{TweqAxisLimits, TweqJoint};
    fn fixture() -> (World, EntityId, JointTweq) {
        let mut world = World::new();
        let entity = world.add_entity((
            PropTweqJointsConfig {
                halt: TweqHalt::Continue,
                primary_joint: 0,
                joints: std::array::from_fn(|_| TweqJoint {
                    curve: 0,
                    animation_config: Config::empty(),
                    limits: TweqAxisLimits {
                        rate: 10.0,
                        low: -30.0,
                        high: 30.0,
                    },
                }),
            },
            PropTweqJointsState {
                animation_state: State::ON,
                joints: [State::ON; 6],
            },
        ));
        let tween = JointTweq::from_world(&world, entity);
        (world, entity, tween)
    }
    #[test]
    fn authored_rate_is_per_100ms_and_bounces_at_the_limits() {
        let (world, entity, mut tween) = fixture();
        tween.update(entity, &world, 0.1);
        assert_eq!(tween.values, [10.0; 6]);
        tween.update(entity, &world, 0.25);
        assert_eq!(tween.values, [30.0; 6]);
        tween.update(entity, &world, 0.1);
        assert_eq!(tween.values, [20.0; 6]);
    }
    #[test]
    fn saved_reversing_joint_resumes_its_phase_and_paused_ticks_do_not_advance() {
        let (world, entity, mut tween) = fixture();
        tween.update(entity, &world, 0.35);
        let saved = serde_json::to_value(&tween).unwrap();
        let mut restored: JointTweq = serde_json::from_value(saved.clone()).unwrap();
        restored.update(entity, &world, 0.0);
        assert_eq!(serde_json::to_value(&restored).unwrap(), saved);
        tween.update(entity, &world, 0.12);
        restored.update(entity, &world, 0.12);
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&tween).unwrap()
        );
        assert_eq!(restored.values, [18.0; 6]);
    }
}
