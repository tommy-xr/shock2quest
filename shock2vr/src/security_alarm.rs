//! The station security alarm.
//!
//! A camera that positively identifies the player raises an alarm for an
//! authored duration. The alarm is **refcounted** - overlapping alarms stack,
//! and the HUD badge appears on the 0 -> 1 transition and disappears on 1 -> 0
//! - and it carries a single deadline; when the time runs out the alarm
//! disables itself.
//!
//! An active alarm puts the station's ecologies into their alert tier
//! (`P$EcoState` = 2), which is what raises spawn populations; the connection
//! is by scanning ecology objects, not by links, because an alarm has no
//! links. Standing security down (a used security computer, or the deadline
//! expiring) posts each alerted ecology a `Reset`, and each ecology's own
//! reset clears its alert tier and the devices on its switch links.
//!
//! The state is presentation-agnostic: [`SecurityAlarm::status`] reports it,
//! and the flat HUD draws the badge and its countdown. (Only the flat
//! presentation renders it today.)

use dark::properties::{PropEcoState, PropEcology};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, Unique, UniqueView, View, World};

use crate::scripts::trigger_ecology::ECOLOGY_STATE_ALERT;
use crate::scripts::{Effect, Message, MessagePayload};
use crate::time::Time;

/// Every ecology object in the world, alerted or not - the alarm reaches them
/// by state, not by links.
fn ecologies(world: &World) -> Vec<(EntityId, i32)> {
    let (Ok(ecologies), Ok(states)) = (
        world.borrow::<View<PropEcology>>(),
        world.borrow::<View<PropEcoState>>(),
    ) else {
        return Vec::new();
    };
    ecologies
        .iter()
        .with_id()
        // Most ecologies author no explicit state; they behave as Normal until
        // their first transition creates the component.
        .map(|(entity, _)| (entity, states.get(entity).map(|state| state.0).unwrap_or(0)))
        .collect()
}

/// Every security ecology currently in its alert tier.
pub fn alerted_ecologies(world: &World) -> Vec<EntityId> {
    ecologies(world)
        .into_iter()
        .filter(|(_, state)| *state == ECOLOGY_STATE_ALERT)
        .map(|(entity, _)| entity)
        .collect()
}

/// The alert duration a camera's linked security ecology authors, in seconds
/// (its alert-tier recovery). Zero when nothing linked authors one.
pub fn authored_alarm_seconds(world: &World, camera: EntityId) -> f32 {
    let Ok(ecologies) = world.borrow::<View<PropEcology>>() else {
        return 0.0;
    };
    crate::scripts::script_util::get_all_switch_links(world, camera)
        .into_iter()
        .filter_map(|linked| ecologies.get(linked).ok())
        .map(|ecology| ecology.recovery_seconds[ECOLOGY_STATE_ALERT as usize])
        .fold(0.0f32, f32::max)
}

/// Post every alerted ecology a `Reset` - the station-wide security
/// stand-down. `from` is whatever did it (a used security computer); the
/// deadline running out has no source, so an ecology resets itself.
fn stand_down(world: &World, from: Option<EntityId>) -> Vec<Effect> {
    alerted_ecologies(world)
        .into_iter()
        .map(|ecology| Effect::Send {
            msg: Message {
                to: ecology,
                payload: MessagePayload::Reset {
                    from: from.unwrap_or(ecology),
                },
            },
        })
        .collect()
}

/// What a presentation (or a debug client) needs to draw the alarm.
///
/// Published into the world every frame so both HUD paths - the flat overlay
/// and the VR forearm - read the same state without either of them owning it.
#[derive(Unique, Debug, Clone, Copy, PartialEq, Default)]
pub struct SecurityAlarmStatus {
    /// How many alarms are outstanding; the badge shows while this is nonzero.
    pub count: u32,
    /// Seconds left on the alarm's deadline, floored at zero.
    pub seconds_remaining: f32,
}

/// The alarm as the world last saw it - what a HUD draws from.
pub fn status(world: &World) -> SecurityAlarmStatus {
    world
        .borrow::<UniqueView<SecurityAlarmStatus>>()
        .map(|status| *status)
        .unwrap_or_default()
}

impl SecurityAlarmStatus {
    pub fn active(&self) -> bool {
        self.count > 0
    }

    /// The countdown a HUD should draw, or `None` while security is calm.
    pub fn hud_seconds(&self) -> Option<f32> {
        self.active().then_some(self.seconds_remaining)
    }
}

/// The refcounted station alarm. Owned by the mission and ticked once per
/// frame; the ecologies it drives hold the durable state, so nothing here
/// needs saving.
#[derive(Default)]
pub struct SecurityAlarm {
    count: u32,
    seconds_remaining: f32,
}

impl SecurityAlarm {
    /// Raise an alarm for `seconds`. Overlapping alarms stack: the count rises
    /// and the deadline extends to whichever runs longest. Only the 0 -> 1
    /// transition alerts the ecologies.
    pub fn add(&mut self, world: &World, seconds: f32) -> Vec<Effect> {
        if seconds <= 0.0 {
            return Vec::new();
        }
        self.seconds_remaining = self.seconds_remaining.max(seconds);
        self.count += 1;
        if self.count > 1 {
            return Vec::new();
        }
        ecologies(world)
            .into_iter()
            .filter(|(_, state)| *state != ECOLOGY_STATE_ALERT)
            .map(|(entity_id, _)| Effect::SetEcologyState {
                entity_id,
                state: ECOLOGY_STATE_ALERT,
            })
            .collect()
    }

    /// Stand security down completely (a used security computer, or the
    /// deadline running out): the count drops to zero and every alerted
    /// ecology is reset.
    pub fn disable(&mut self, world: &World, from: Option<EntityId>) -> Vec<Effect> {
        if self.count == 0 {
            return Vec::new();
        }
        self.count = 0;
        self.seconds_remaining = 0.0;
        stand_down(world, from)
    }

    /// Run the deadline down; the alarm disables itself when it expires.
    pub fn update(&mut self, world: &World, time: &Time) -> Vec<Effect> {
        if self.count == 0 {
            return Vec::new();
        }
        self.seconds_remaining -= time.elapsed.as_secs_f32();
        if self.seconds_remaining > 0.0 {
            return Vec::new();
        }
        self.disable(world, None)
    }

    pub fn status(&self) -> SecurityAlarmStatus {
        SecurityAlarmStatus {
            count: self.count,
            seconds_remaining: self.seconds_remaining.max(0.0),
        }
    }

    /// Publish the current status into the world, where every presentation
    /// reads it.
    pub fn publish(&self, world: &World) {
        world.add_unique(self.status());
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

    fn ecology(recovery: f32) -> PropEcology {
        PropEcology {
            period_seconds: 15.0,
            min_count: [0; 3],
            max_count: [0; 3],
            recovery_seconds: [0.0, 0.0, recovery],
            random_chance: [0; 3],
        }
    }

    /// A world with one ecology in the given state, authored with medsci1's
    /// 120 s alert recovery.
    fn ecology_world(state: i32) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let ecology_id = world.add_entity((PropEcoState(state), ecology(120.0)));
        let source = world.add_entity(());
        (world, ecology_id, source)
    }

    fn alerts(effects: &[Effect]) -> Vec<EntityId> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::SetEcologyState { entity_id, state } if *state == ECOLOGY_STATE_ALERT => {
                    Some(*entity_id)
                }
                _ => None,
            })
            .collect()
    }

    fn resets(effects: &[Effect]) -> Vec<EntityId> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } if matches!(msg.payload, MessagePayload::Reset { .. }) => {
                    Some(msg.to)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn raising_an_alarm_alerts_the_ecologies_and_starts_the_countdown() {
        let (world, ecology_id, source) = ecology_world(0);
        let mut alarm = SecurityAlarm::default();
        assert!(!alarm.status().active());

        let effects = alarm.add(&world, 120.0);
        assert_eq!(alerts(&effects), vec![ecology_id]);
        assert_eq!(
            alarm.status(),
            SecurityAlarmStatus {
                count: 1,
                seconds_remaining: 120.0
            }
        );
        let _ = source;
    }

    #[test]
    fn a_zero_length_alarm_is_no_alarm() {
        let (world, _, _) = ecology_world(0);
        let mut alarm = SecurityAlarm::default();
        assert!(alarm.add(&world, 0.0).is_empty());
        assert!(!alarm.status().active());
    }

    #[test]
    fn overlapping_alarms_are_refcounted_and_extend_the_deadline() {
        let (world, _, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.add(&world, 30.0);
        // The second alarm neither re-alerts the ecologies nor shortens the
        // deadline; it only raises the count.
        let second = alarm.add(&world, 120.0);
        assert!(alerts(&second).is_empty());
        assert_eq!(alarm.status().count, 2);
        assert_eq!(alarm.status().seconds_remaining, 120.0);

        // The shorter alarm's window passing does not end the alert.
        assert!(alarm.update(&world, &time(60.0)).is_empty());
        assert!(alarm.status().active());
    }

    #[test]
    fn the_deadline_stands_security_down() {
        let (world, ecology_id, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.add(&world, 120.0);

        assert!(alarm.update(&world, &time(119.0)).is_empty());
        assert!(alarm.status().active());
        assert_eq!(alarm.status().seconds_remaining, 1.0);

        let expiry = alarm.update(&world, &time(2.0));
        assert_eq!(resets(&expiry), vec![ecology_id]);
        assert_eq!(
            alarm.status(),
            SecurityAlarmStatus {
                count: 0,
                seconds_remaining: 0.0
            }
        );
        // ...and it stays down.
        assert!(alarm.update(&world, &time(1.0)).is_empty());
    }

    #[test]
    fn standing_down_early_clears_every_outstanding_alarm() {
        let (world, ecology_id, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.add(&world, 120.0);
        alarm.add(&world, 120.0);

        let effects = alarm.disable(&world, Some(source));
        assert_eq!(resets(&effects), vec![ecology_id]);
        assert!(!alarm.status().active());
        // A stand-down with nothing to stand down is inert.
        assert!(alarm.disable(&world, Some(source)).is_empty());
    }

    #[test]
    fn the_alarm_duration_comes_from_the_linked_ecology() {
        use dark::properties::{Link, Links, ToLink, WrappedEntityId};

        let mut world = World::new();
        let ecology_id = world.add_entity((PropEcoState(0), ecology(120.0)));
        let camera = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 71,
                to_entity_id: Some(WrappedEntityId(ecology_id)),
                link: Link::SwitchLink,
            }],
        });
        assert_eq!(authored_alarm_seconds(&world, camera), 120.0);

        // A camera linked to nothing that authors an alert recovery raises no
        // alarm at all.
        let unlinked = world.add_entity(());
        assert_eq!(authored_alarm_seconds(&world, unlinked), 0.0);
    }
}
