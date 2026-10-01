//! Secondary motion for the amp's authored cable; never moves the held amp.
use cgmath::{InnerSpace, Rotation, Vector3, vec3};
use dark::properties::PropModelName;
use shipyard::{Component, EntityId, Get, IntoIter, View, ViewMut, World};

use crate::{
    input_context::{Hand, InputContext},
    runtime_props::RuntimePropObjectArticulation,
    scripts::Effect,
    weapon_recoil::Spring,
};

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct CableSway {
    previous: Option<Vector3<f32>>,
    owner: Option<usize>,
    spring: Spring,
}

impl CableSway {
    fn step(&mut self, hand: &Hand, tracked: bool, dt: f32) -> f32 {
        let valid = tracked
            && [
                hand.position.x,
                hand.position.y,
                hand.position.z,
                hand.rotation.s,
                hand.rotation.v.x,
                hand.rotation.v.y,
                hand.rotation.v.z,
            ]
            .into_iter()
            .all(f32::is_finite)
            && hand.rotation.magnitude2() > 1e-8;
        if !valid {
            self.previous = None;
            self.spring = Spring::default();
            return 0.0;
        }
        // A point slightly below/in front of the grip responds to both hand
        // translation and wrist turns. Player-space samples ignore teleport and
        // artificial locomotion; recovery/equip must not generate an impulse.
        let rotation = hand.rotation.normalize();
        let sample = hand.position + rotation.rotate_vector(vec3(0.0, -0.15, -0.10));
        if !dt.is_finite() || dt > 0.25 {
            self.previous = Some(sample);
            self.spring = Spring::default();
            return 0.0;
        }
        if dt <= 0.0 {
            return self.spring.position;
        }
        let delta = self.previous.replace(sample).map(|p| sample - p);
        if delta.is_some_and(|d| d.magnitude() > 0.3) {
            self.spring = Spring::default();
        } else if let Some(delta) = delta {
            // The cable swings across the hand. Degrees and gain are VR tuning;
            // Nightdive used locomotion bob to drive this same rotational joint.
            let lateral_speed = rotation.conjugate().rotate_vector(delta).x / dt;
            let target = (-lateral_speed * 18.0).clamp(-18.0, 18.0);
            self.spring.step_toward(dt, target);
        } else {
            self.spring.step_toward(dt, 0.0);
        }
        self.spring.position.clamp(-18.0, 18.0)
    }
}

/// Pause skips scene updates entirely, so discard its last hand sample without
/// advancing/resetting the suspended cable pose. Resume samples a new baseline.
pub(crate) fn suspend(world: &World) {
    if let Ok(mut sways) = world.borrow::<ViewMut<CableSway>>() {
        for sway in (&mut sways).iter() {
            sway.previous = None;
        }
    }
}

pub(crate) fn advance(
    world: &mut World,
    held: (Option<EntityId>, Option<EntityId>),
    input: &InputContext,
    dt: f32,
) -> Vec<Effect> {
    if !crate::mission::mission_core::presentation_is_vr(world) {
        return Vec::new();
    }
    let mut effects = Vec::new();
    for (index, entity) in [held.0, held.1].into_iter().enumerate() {
        let Some(entity) = entity else {
            continue;
        };
        // The amp intentionally retains its authored hand, so it does not carry
        // RuntimePropGloveWeapon. Check the actually held model and cable rig.
        let enabled = world
            .borrow::<(View<PropModelName>, View<RuntimePropObjectArticulation>)>()
            .is_ok_and(|(names, rigs)| {
                names
                    .get(entity)
                    .is_ok_and(|n| n.0.eq_ignore_ascii_case("amp_h"))
                    && rigs.get(entity).is_ok_and(|rig| {
                        rig.0
                            .joints
                            .iter()
                            .any(|j| j.parameter == 0 && j.motion_type == 1)
                    })
            });
        if !enabled {
            continue;
        }
        let mut sway = world
            .borrow::<View<CableSway>>()
            .ok()
            .and_then(|v| v.get(entity).ok().copied())
            .unwrap_or_default();
        if sway.owner != Some(index) {
            sway = CableSway {
                owner: Some(index),
                ..Default::default()
            };
        }
        let hand = if index == 0 {
            &input.left_hand
        } else {
            &input.right_hand
        };
        let value = sway.step(hand, input.pose_tracking.is_none_or(|p| p.hands[index]), dt);
        world.add_component(entity, sway);
        effects.push(Effect::SetObjectParameters {
            entity_id: entity,
            parameters: vec![(0, value)],
        });
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Quaternion, Rotation3};

    #[test]
    fn movement_is_bounded_and_settles_at_multiple_frame_rates() {
        for hz in [30, 60, 120] {
            let mut sway = CableSway::default();
            let mut hand = Hand::default();
            hand.rotation = Quaternion::from_angle_y(Deg(0.0));
            let dt = 1.0 / hz as f32;
            assert_eq!(sway.step(&hand, true, dt), 0.0);
            let mut peak = 0.0_f32;
            for f in 1..=hz {
                hand.position.x = 0.2 * (f as f32 * dt * std::f32::consts::TAU).sin();
                peak = peak.max(sway.step(&hand, true, dt).abs());
            }
            assert!(peak > 2.0 && peak <= 18.0, "{hz}Hz peak {peak}");
            for _ in 0..hz * 4 {
                sway.step(&hand, true, dt);
            }
            assert!(sway.spring.position.abs() < 0.001);
        }
    }

    #[test]
    fn tracking_recovery_pause_and_discontinuities_do_not_kick_the_cable() {
        let mut sway = CableSway::default();
        let mut hand = Hand::default();
        hand.rotation = Quaternion::from_angle_y(Deg(0.0));
        let dt = 1.0 / 60.0;
        sway.step(&hand, true, dt);
        hand.position.x += 0.05;
        let moving = sway.step(&hand, true, dt);
        assert!(moving.abs() > 0.0);
        assert_eq!(sway.step(&hand, true, 0.0), moving);
        assert_eq!(sway.step(&hand, false, dt), 0.0);
        hand.position.x = 10.0;
        assert_eq!(sway.step(&hand, true, dt), 0.0);
        hand.position.x = 20.0;
        assert_eq!(sway.step(&hand, true, dt), 0.0);
        assert_eq!(sway.step(&hand, true, 1.0), 0.0);
        hand.rotation.s = f32::NAN;
        assert_eq!(sway.step(&hand, true, dt), 0.0);
    }

    #[test]
    fn wrist_turns_also_move_the_cable() {
        let mut sway = CableSway::default();
        let mut hand = Hand::default();
        hand.rotation = Quaternion::from_angle_y(Deg(0.0));
        sway.step(&hand, true, 1.0 / 60.0);
        hand.rotation = Quaternion::from_angle_y(Deg(10.0));
        assert!(sway.step(&hand, true, 1.0 / 60.0).abs() > 0.01);
    }
}
