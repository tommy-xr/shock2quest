//! The station security alarm.
//!
//! A camera that positively identifies the player raises an alarm for an
//! authored duration. The alarm is **refcounted** - overlapping alarms stack,
//! and the HUD badge appears on the 0 -> 1 transition and disappears on 1 -> 0
//! - and it carries a single deadline; when the time runs out the alarm
//! disables itself.
//!
//! An alarm alerts the station's ecologies, which is what raises spawn
//! populations - their alert tier (`P$EcoState` = 2) is where the alert
//! actually lives. The alarm reaches them by scanning ecology objects rather
//! than by links, because an alarm has none, and it *alarms* them by message
//! so each arms its own authored recovery and keeps its own reasons for
//! refusing. Standing security down (a used security computer, or the window
//! running out) posts each alerted ecology a `Reset`, and each ecology's own
//! reset clears its tier and the devices on its switch links.
//!
//! The state is presentation-agnostic: [`status`] publishes it into the world
//! and every presentation and the debug surface read it from there, so none of
//! them can disagree about whether the badge is up.

use dark::properties::{PropEcoState, PropEcology};
use shipyard::{
    EntityId, Get, IntoIter, IntoWithId, Unique, UniqueView, UniqueViewMut, View, World,
};

use crate::scripts::trigger_ecology::{ECOLOGY_STATE_ALERT, ECOLOGY_STATE_NORMAL};
use crate::scripts::{Effect, Message, MessagePayload};
use crate::time::Time;

/// `recovery_seconds` / `min_count` / ... are indexed by ecology tier.
const ALERT_TIER: usize = ECOLOGY_STATE_ALERT as usize;

/// Every ecology object in the world, alerted or not - the alarm reaches them
/// by state, not by links.
fn ecologies(world: &World) -> Vec<(EntityId, i32)> {
    let Ok(ecologies) = world.borrow::<View<PropEcology>>() else {
        return Vec::new();
    };
    // Most ecologies author no explicit state, and until the first transition
    // in a level creates one the storage does not exist at all - so a missing
    // state, and a missing *storage*, both read as Normal.
    let states = world.borrow::<View<PropEcoState>>().ok();
    ecologies
        .iter()
        .with_id()
        .map(|(entity, _)| {
            let state = states
                .as_ref()
                .and_then(|states| states.get(entity).ok().map(|state| state.0))
                .unwrap_or(ECOLOGY_STATE_NORMAL);
            (entity, state)
        })
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

/// Ecologies that will take a security alarm: in the normal tier, with an
/// authored alert profile to recover from. These are `trigger_ecology`'s own
/// conditions for accepting an `Alarm` - a hacked ecology ignores security,
/// and one with no alert profile would expire the moment it entered.
fn alarmable_ecologies(world: &World) -> Vec<EntityId> {
    let Ok(props) = world.borrow::<View<PropEcology>>() else {
        return Vec::new();
    };
    ecologies(world)
        .into_iter()
        .filter(|(entity, state)| {
            *state == ECOLOGY_STATE_NORMAL
                && props
                    .get(*entity)
                    .is_ok_and(|ecology| ecology.recovery_seconds[ALERT_TIER] > 0.0)
        })
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
        .map(|ecology| ecology.recovery_seconds[ALERT_TIER])
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
    /// Seconds left on a hacked security console's window, during which the
    /// level's cameras cannot see the player. Zero when they can.
    pub cameras_blind_seconds: f32,
}

/// Whether a hacked security console currently hides the player from the
/// level's cameras. Only the camera vision channel is affected - a blinded
/// camera is still alive, and every other AI sees the player normally.
pub fn cameras_are_blind(world: &World) -> bool {
    status(world).cameras_blind_seconds > 0.0
}

/// The alarm as last published into the world - the single value every
/// presentation and the debug surface draw from, so none of them can disagree
/// about whether the badge is up.
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
    /// Whether the alert has actually reached the ecologies yet. The `Alarm`
    /// messages are delivered a frame or two after the alarm is raised, so
    /// "no ecology is alerted" only means the alert is over once it has been
    /// seen to start.
    alert_landed: bool,
    /// What is left of a hacked console's camera-blindness window. The alarm
    /// owns it because the two are mutually exclusive: raising an alarm ends
    /// the blindness, exactly as standing security down ends the alarm.
    ///
    /// Like the alarm around it, this is runtime-only: a save taken mid-window
    /// reloads with the cameras seeing again (the alarm reloads the same way,
    /// derived from the ecologies it drives). Both would need a durable home
    /// to survive, and a console is cheap to hack again.
    cameras_blind_seconds: f32,
}

impl SecurityAlarm {
    /// Raise an alarm for `seconds`, alerting every ecology that will take one.
    ///
    /// The ecologies are alarmed by message, not by writing their state
    /// directly, so each arms its own authored recovery (which is saved with
    /// it) and keeps its own conditions for refusing. Overlapping alarms raise
    /// the count but do **not** extend the window - the ecologies' own
    /// recovery does not either, and a badge outlasting the alert would lie.
    pub fn add(&mut self, world: &World, seconds: f32) -> Vec<Effect> {
        if seconds <= 0.0 {
            return Vec::new();
        }
        if self.count > 0 {
            // Already up: the alarm stacks, but the window does not move and
            // the alerted ecologies are already alerted.
            self.count += 1;
            self.cameras_blind_seconds = 0.0;
            return Vec::new();
        }
        // An alarm nothing holds is no alarm - the badge must not show a
        // station-wide alert no ecology is actually running. An ecology that
        // is *already* alerted counts: the camera that alarms its own linked
        // ecology over its switch link gets there first.
        let takers = alarmable_ecologies(world);
        if takers.is_empty() && alerted_ecologies(world).is_empty() {
            return Vec::new();
        }
        self.seconds_remaining = seconds;
        self.count = 1;
        self.alert_landed = false;
        // An alarm and a hacked console are mutually exclusive: security being
        // up is exactly what the hack was suppressing.
        self.cameras_blind_seconds = 0.0;
        takers
            .into_iter()
            .map(|ecology| Effect::Send {
                msg: Message {
                    to: ecology,
                    payload: MessagePayload::Alarm { from: ecology },
                },
            })
            .collect()
    }

    /// Stand security down completely (a used security computer, or the
    /// deadline running out): the count drops to zero and every alerted
    /// ecology is reset.
    ///
    /// The reset scan does not depend on this alarm's own count: a level whose
    /// ecologies are alerted with no alarm behind them (a save taken mid-alarm
    /// reloads that way, since the alarm itself is derived) must still be
    /// clearable at a console.
    pub fn disable(&mut self, world: &World, from: Option<EntityId>) -> Vec<Effect> {
        self.count = 0;
        self.seconds_remaining = 0.0;
        self.alert_landed = false;
        stand_down(world, from)
    }

    /// Hide the player from the level's cameras for `seconds` - a hacked
    /// security console. The longer window wins, so a second hack cannot cut
    /// the first one short.
    pub fn blind_cameras(&mut self, seconds: f32) {
        if seconds <= 0.0 {
            return;
        }
        self.cameras_blind_seconds = self.cameras_blind_seconds.max(seconds);
    }

    /// Run the deadline down. The alarm ends when the window runs out - or as
    /// soon as the ecologies have stood themselves down, since they, not this
    /// countdown, are where the alert actually lives.
    pub fn update(&mut self, world: &World, time: &Time) -> Vec<Effect> {
        if self.cameras_blind_seconds > 0.0 {
            self.cameras_blind_seconds =
                (self.cameras_blind_seconds - time.elapsed.as_secs_f32()).max(0.0);
        }
        if self.count == 0 {
            return Vec::new();
        }
        self.seconds_remaining -= time.elapsed.as_secs_f32();
        let alerted = !alerted_ecologies(world).is_empty();
        self.alert_landed |= alerted;
        if self.seconds_remaining > 0.0 && (alerted || !self.alert_landed) {
            return Vec::new();
        }
        self.disable(world, None)
    }

    pub fn status(&self) -> SecurityAlarmStatus {
        SecurityAlarmStatus {
            count: self.count,
            seconds_remaining: self.seconds_remaining.max(0.0),
            cameras_blind_seconds: self.cameras_blind_seconds.max(0.0),
        }
    }

    /// Publish the current status into the world, where every presentation
    /// reads it.
    pub fn publish(&self, world: &World) {
        let status = self.status();
        // `add_unique` inserts but does not replace, so an existing value has
        // to be assigned through - otherwise every reader is stuck on the
        // first frame's (calm) status forever.
        match world.borrow::<UniqueViewMut<SecurityAlarmStatus>>() {
            Ok(mut published) => *published = status,
            Err(_) => world.add_unique(status),
        }
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

    /// A world with one ecology in the given tier, authored with medsci1's
    /// 120 s alert recovery, plus an unrelated source object.
    fn ecology_world(state: i32) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let ecology_id = world.add_entity((PropEcoState(state), ecology(120.0)));
        let source = world.add_entity(());
        (world, ecology_id, source)
    }

    /// Who the effects alarm.
    fn alarms(effects: &[Effect]) -> Vec<EntityId> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Send { msg } if matches!(msg.payload, MessagePayload::Alarm { .. }) => {
                    Some(msg.to)
                }
                _ => None,
            })
            .collect()
    }

    /// Who the effects reset.
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
    fn raising_an_alarm_alarms_the_ecologies_and_starts_the_countdown() {
        let (world, ecology_id, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        assert!(!alarm.status().active());

        let effects = alarm.add(&world, 120.0);
        // Alarmed by message, so each ecology arms its own authored recovery
        // and keeps its own reasons for refusing.
        assert_eq!(alarms(&effects), vec![ecology_id]);
        assert_eq!(
            alarm.status(),
            SecurityAlarmStatus {
                count: 1,
                seconds_remaining: 120.0,
                ..SecurityAlarmStatus::default()
            }
        );
    }

    #[test]
    fn a_zero_length_alarm_is_no_alarm() {
        let (world, _, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        assert!(alarm.add(&world, 0.0).is_empty());
        assert!(!alarm.status().active());
    }

    #[test]
    fn an_alarm_nothing_will_take_does_not_raise_the_badge() {
        // A hacked ecology ignores security alarms, and one with no authored
        // alert profile has nothing to enter - neither can hold an alert, so
        // neither may light the badge.
        for (state, recovery) in [(1, 120.0), (ECOLOGY_STATE_NORMAL, 0.0)] {
            let mut world = World::new();
            world.add_entity((PropEcoState(state), ecology(recovery)));
            let mut alarm = SecurityAlarm::default();
            assert!(alarm.add(&world, 120.0).is_empty());
            assert!(!alarm.status().active());
        }
    }

    #[test]
    fn an_already_alerted_ecology_still_raises_the_badge() {
        // The camera alarms its own linked ecology over its switch link before
        // the station alarm is raised, so by then there is nothing left to
        // alarm - but the alert is real and the badge must show it.
        let (world, _, _) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        let effects = alarm.add(&world, 120.0);
        assert!(alarms(&effects).is_empty());
        assert_eq!(alarm.status().seconds_remaining, 120.0);
        assert!(alarm.status().active());
    }

    #[test]
    fn overlapping_alarms_stack_without_moving_the_window() {
        let (world, _, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        alarm.add(&world, 120.0);

        // A second alarm neither re-alarms the ecologies nor extends the
        // window - the ecologies' own recovery does not extend either, and a
        // badge outlasting the alert would lie.
        let second = alarm.add(&world, 300.0);
        assert!(alarms(&second).is_empty());
        assert_eq!(alarm.status().count, 2);
        assert_eq!(alarm.status().seconds_remaining, 120.0);
    }

    #[test]
    fn the_window_running_out_stands_security_down() {
        let (world, ecology_id, _) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.count = 1;
        alarm.seconds_remaining = 120.0;

        assert!(alarm.update(&world, &time(119.0)).is_empty());
        assert!(alarm.status().active());
        assert_eq!(alarm.status().seconds_remaining, 1.0);

        let expiry = alarm.update(&world, &time(2.0));
        assert_eq!(resets(&expiry), vec![ecology_id]);
        assert_eq!(
            alarm.status(),
            SecurityAlarmStatus {
                count: 0,
                seconds_remaining: 0.0,
                ..SecurityAlarmStatus::default()
            }
        );
        // ...and it stays down.
        assert!(alarm.update(&world, &time(1.0)).is_empty());
    }

    #[test]
    fn ecologies_standing_themselves_down_ends_the_alarm() {
        // The ecologies, not this countdown, are where the alert lives: if
        // they recover on their own the badge must not linger.
        let (mut world, ecology_id, _) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.count = 1;
        alarm.seconds_remaining = 120.0;
        assert!(alarm.update(&world, &time(1.0)).is_empty());

        // The ecology's own recovery expires and it returns to Normal.
        world.add_component(ecology_id, PropEcoState(ECOLOGY_STATE_NORMAL));
        assert!(alarm.update(&world, &time(1.0)).is_empty());
        assert!(!alarm.status().active());
    }

    #[test]
    fn the_badge_survives_the_frames_before_the_alert_lands() {
        // The `Alarm` messages are delivered after the alarm is raised, so
        // for a frame or two no ecology is alerted yet - the alarm must not
        // read that as "already over" and stand itself down.
        let (world, _, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        alarm.add(&world, 120.0);

        for _ in 0..3 {
            assert!(alarm.update(&world, &time(1.0 / 60.0)).is_empty());
            assert!(alarm.status().active());
        }
    }

    #[test]
    fn a_hacked_console_blinds_the_cameras_until_its_window_runs_out() {
        let (world, _, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        assert_eq!(alarm.status().cameras_blind_seconds, 0.0);

        alarm.blind_cameras(120.0);
        assert_eq!(alarm.status().cameras_blind_seconds, 120.0);
        // A shorter second hack cannot cut the running window short.
        alarm.blind_cameras(30.0);
        assert_eq!(alarm.status().cameras_blind_seconds, 120.0);

        alarm.update(&world, &time(119.0));
        assert_eq!(alarm.status().cameras_blind_seconds, 1.0);
        alarm.update(&world, &time(2.0));
        assert_eq!(alarm.status().cameras_blind_seconds, 0.0);
    }

    #[test]
    fn raising_an_alarm_ends_the_blindness() {
        // Security being up is exactly what the hack was suppressing, so the
        // two can never be true at once.
        let (world, _, _) = ecology_world(ECOLOGY_STATE_NORMAL);
        let mut alarm = SecurityAlarm::default();
        alarm.blind_cameras(120.0);

        alarm.add(&world, 120.0);
        assert_eq!(alarm.status().cameras_blind_seconds, 0.0);

        // ...including an alarm that only stacks onto one already up.
        alarm.blind_cameras(120.0);
        alarm.add(&world, 120.0);
        assert_eq!(alarm.status().cameras_blind_seconds, 0.0);
    }

    #[test]
    fn standing_security_down_leaves_the_hacked_window_running() {
        // A console hack does both at once - the stand-down must not undo the
        // blindness the same win just bought.
        let (world, _, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.blind_cameras(120.0);
        alarm.disable(&world, Some(source));
        assert_eq!(alarm.status().cameras_blind_seconds, 120.0);
    }

    #[test]
    fn standing_down_early_clears_every_outstanding_alarm() {
        let (world, ecology_id, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        alarm.count = 2;
        alarm.seconds_remaining = 120.0;

        let effects = alarm.disable(&world, Some(source));
        assert_eq!(resets(&effects), vec![ecology_id]);
        assert!(!alarm.status().active());
    }

    #[test]
    fn a_console_clears_alerted_ecologies_even_with_no_alarm_of_our_own() {
        // A save taken mid-alarm reloads with the ecologies alerted and no
        // alarm behind them - the console must still stand them down.
        let (world, ecology_id, source) = ecology_world(ECOLOGY_STATE_ALERT);
        let mut alarm = SecurityAlarm::default();
        assert_eq!(
            resets(&alarm.disable(&world, Some(source))),
            vec![ecology_id]
        );

        // With nothing alerted it is inert.
        let (calm, _, source) = ecology_world(ECOLOGY_STATE_NORMAL);
        assert!(alarm.disable(&calm, Some(source)).is_empty());
    }

    #[test]
    fn the_alarm_duration_comes_from_the_linked_ecology() {
        use dark::properties::{Link, Links, ToLink, WrappedEntityId};

        let mut world = World::new();
        let ecology_id = world.add_entity((PropEcoState(ECOLOGY_STATE_NORMAL), ecology(120.0)));
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
