//! Station security alert feedback.
//!
//! A security ecology enters its alert column when a camera raises an `Alarm`
//! (`camera_alert` -> `trigger_ecology`), and leaves it when the authored
//! recovery expires or a `Reset` arrives. That `P$EcoState` is the whole
//! station's alert state, so the retail feedback - a blinking HUD badge and a
//! looping "Potential threat detected." warning - is driven from it here
//! rather than from any one AI's alertness.
//!
//! The state is presentation-agnostic: [`SecurityAlert::badge_visible`] says
//! whether the badge is lit this frame, and the flat HUD draws it. (Only the
//! flat presentation renders it today.)

use dark::properties::{PropEcoState, PropEcology};
use engine::audio::AudioHandle;
use shipyard::{Get, IntoIter, IntoWithId, View, World};

use crate::scripts::Effect;
use crate::time::Time;

/// `P$EcoState` value for the alert column - see `scripts::trigger_ecology`.
const ECOLOGY_STATE_ALERT: i32 = 2;

/// Xerxes' "Potential threat detected." (res/snd2/vTriggers), the warning the
/// station repeats while security is alerted.
const WARNING_SAMPLE: &str = "xxyal001";

/// Seconds between repeats of the warning while the alert holds.
const WARNING_PERIOD_SECONDS: f32 = 10.0;

/// Badge blink cycle; the badge is lit for the first half of each cycle.
const BLINK_PERIOD_SECONDS: f32 = 1.0;

/// Whether any security ecology is currently in its alert column.
///
/// `P$EcoState` is only meaningful on an ecology, so both properties are
/// required - an unrelated object carrying a stray state must not alert the
/// whole station.
pub fn is_alert_active(world: &World) -> bool {
    let (Ok(states), Ok(ecologies)) = (
        world.borrow::<View<PropEcoState>>(),
        world.borrow::<View<PropEcology>>(),
    ) else {
        return false;
    };
    states
        .iter()
        .with_id()
        .any(|(entity, state)| state.0 == ECOLOGY_STATE_ALERT && ecologies.get(entity).is_ok())
}

/// Drives the security-alert feedback: the badge blink phase and the looping
/// warning. Owned by the mission and ticked once per frame; the alert itself
/// lives in the world (`P$EcoState`), so nothing here needs to be saved.
#[derive(Default)]
pub struct SecurityAlert {
    active: bool,
    /// Seconds the current alert has been up, for the blink phase.
    elapsed: f32,
    seconds_until_warning: f32,
    /// The most recent warning play, so clearing the alert can cut a line
    /// that is still speaking.
    warning: Option<AudioHandle>,
}

impl SecurityAlert {
    /// Advance the feedback for one frame, returning the audio effects it
    /// wants applied.
    pub fn update(&mut self, world: &World, time: &Time) -> Vec<Effect> {
        let active = is_alert_active(world);
        if active != self.active {
            self.active = active;
            self.elapsed = 0.0;
            // A fresh alert speaks immediately; a cleared one goes quiet.
            self.seconds_until_warning = 0.0;
            if !active {
                return self.stop_warning().into_iter().collect();
            }
        }
        if !active {
            return Vec::new();
        }

        let delta = time.elapsed.as_secs_f32();
        self.elapsed += delta;
        self.seconds_until_warning -= delta;
        if self.seconds_until_warning > 0.0 {
            return Vec::new();
        }
        self.seconds_until_warning = WARNING_PERIOD_SECONDS;
        let handle = AudioHandle::new();
        self.warning = Some(handle.clone());
        vec![Effect::PlaySound {
            handle,
            name: WARNING_SAMPLE.to_owned(),
            // A station-wide announcement, not a world emitter: constant
            // volume, no position.
            source: None,
            spatial: false,
        }]
    }

    /// Whether the alert badge is lit this frame.
    pub fn badge_visible(&self) -> bool {
        self.active && self.elapsed % BLINK_PERIOD_SECONDS < BLINK_PERIOD_SECONDS / 2.0
    }

    fn stop_warning(&mut self) -> Option<Effect> {
        self.warning
            .take()
            .map(|handle| Effect::StopSound { handle })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn time(seconds: f32) -> Time {
        Time {
            elapsed: Duration::from_secs_f32(seconds),
            total: Duration::from_secs_f32(seconds),
        }
    }

    /// A world with one ecology in the given state.
    fn ecology_world(state: i32) -> World {
        let mut world = World::new();
        world.add_entity((
            PropEcoState(state),
            PropEcology {
                period_seconds: 15.0,
                min_count: [0; 3],
                max_count: [0; 3],
                recovery_seconds: [0.0, 0.0, 60.0],
                random_chance: [0; 3],
            },
        ));
        world
    }

    fn plays_warning(effects: &[Effect]) -> bool {
        effects.iter().any(
            |effect| matches!(effect, Effect::PlaySound { name, .. } if name == WARNING_SAMPLE),
        )
    }

    #[test]
    fn an_alerted_ecology_is_the_alert_state() {
        assert!(is_alert_active(&ecology_world(ECOLOGY_STATE_ALERT)));
        assert!(!is_alert_active(&ecology_world(0)));
    }

    #[test]
    fn a_stray_ecostate_on_a_non_ecology_does_not_alert() {
        let mut world = World::new();
        world.add_entity((PropEcoState(ECOLOGY_STATE_ALERT),));
        assert!(!is_alert_active(&world));
    }

    #[test]
    fn the_warning_speaks_on_entry_and_repeats_while_alerted() {
        let world = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alert = SecurityAlert::default();

        assert!(plays_warning(&alert.update(&world, &time(0.016))));
        // Silent until the repeat period elapses.
        assert!(!plays_warning(&alert.update(&world, &time(1.0))));
        assert!(plays_warning(
            &alert.update(&world, &time(WARNING_PERIOD_SECONDS))
        ));
    }

    #[test]
    fn clearing_the_alert_stops_the_warning_and_the_badge() {
        let alerted = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alert = SecurityAlert::default();
        alert.update(&alerted, &time(0.016));
        assert!(alert.badge_visible());

        let calm = ecology_world(0);
        let effects = alert.update(&calm, &time(0.016));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::StopSound { .. }))
        );
        assert!(!alert.badge_visible());
        // ...and stays quiet.
        assert!(alert.update(&calm, &time(0.016)).is_empty());
    }

    #[test]
    fn the_badge_blinks() {
        let world = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alert = SecurityAlert::default();
        alert.update(&world, &time(0.0));
        assert!(alert.badge_visible());
        // Second half of the cycle is dark, and it lights again next cycle.
        alert.update(&world, &time(BLINK_PERIOD_SECONDS * 0.6));
        assert!(!alert.badge_visible());
        alert.update(&world, &time(BLINK_PERIOD_SECONDS * 0.5));
        assert!(alert.badge_visible());
    }

    #[test]
    fn nothing_happens_without_an_alert() {
        let world = ecology_world(0);
        let mut alert = SecurityAlert::default();
        assert!(alert.update(&world, &time(0.016)).is_empty());
        assert!(!alert.badge_visible());
    }
}
