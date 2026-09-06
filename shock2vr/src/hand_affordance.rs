//! What a VR hand could do with whatever it is pointing at, resolved once per
//! hand per frame from the same raycast the trigger and squeeze act on.
//!
//! The light says *whether* the hand can act (green: yes, amber: recognised but
//! refused, red: an attempt just failed); the pre-shape pose says *what* the
//! action is (curl toward a grip, or extend the index toward a press). Two
//! channels, one target - a light that promised an action the input would
//! refuse is exactly the drift AGENTS.md section 3 warns about, so both read
//! this one resolved state.

use shipyard::{EntityId, Get, View, World};

use crate::hand_glove::{HandLight, HandPreshape};

/// How long a hover survives losing its target, in fixed 60 Hz frames. A hand
/// held at the edge of a small collider dips in and out of the ray for a frame
/// at a time; without this the light strobes.
const HOVER_HOLD_FRAMES: u8 = 6;

/// How long a refused attempt shows red, in fixed 60 Hz frames (~250 ms).
const FAILED_PULSE_FRAMES: u8 = 15;

/// Pre-shape weight with the target at arm's length, and with it under the
/// fingertips. The hand prompts harder as it closes in (the UEVR-style
/// distance ramp), but never so hard that an idle hand looks like it is
/// already gripping.
const PRESHAPE_WEIGHT_FAR: f32 = 0.15;
const PRESHAPE_WEIGHT_NEAR: f32 = 0.45;

/// What the hand could do with its current target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HandAffordance {
    /// Nothing actionable in reach.
    #[default]
    None,
    /// A pickup the squeeze would take into the hand.
    Grabbable,
    /// A button, switch, door or scripted pickup the trigger would frob.
    Frobbable,
    /// Recognised, but the hand cannot act on it: locked, or needing a
    /// credential the player has not collected.
    Blocked,
    /// An attempt was refused this frame. Transient - it decays back to the
    /// hover state.
    Failed,
}

impl HandAffordance {
    /// The name reported over HTTP (`/v1/info` -> `player.hand_affordance`).
    pub fn as_str(self) -> &'static str {
        match self {
            HandAffordance::None => "None",
            HandAffordance::Grabbable => "Grabbable",
            HandAffordance::Frobbable => "Frobbable",
            HandAffordance::Blocked => "Blocked",
            HandAffordance::Failed => "Failed",
        }
    }

    /// Eligibility: can the hand act, yes / not here / not that.
    pub fn light(self) -> HandLight {
        match self {
            HandAffordance::None => HandLight::Off,
            HandAffordance::Grabbable | HandAffordance::Frobbable => HandLight::Green,
            HandAffordance::Blocked => HandLight::Amber,
            HandAffordance::Failed => HandLight::Red,
        }
    }

    /// Action: which shape the hand leans into to prompt it. Only the states
    /// the hand can actually act on prompt - a blocked target has no action to
    /// offer.
    fn preshape(self, weight: f32) -> HandPreshape {
        match self {
            HandAffordance::Grabbable => HandPreshape::Grip(weight),
            HandAffordance::Frobbable => HandPreshape::Point(weight),
            _ => HandPreshape::None,
        }
    }
}

/// The per-hand affordance state machine: hysteresis on the hover, and a fixed
/// pulse for a refused attempt.
#[derive(Debug, Clone, Copy, Default)]
pub struct AffordanceTracker {
    hover: HandAffordance,
    hover_weight: f32,
    hold_frames: u8,
    failed_frames: u8,
}

impl AffordanceTracker {
    /// Advance one simulation frame. `observed` is this frame's raw
    /// classification ([`HandAffordance::None`] when the ray hit nothing
    /// actionable) and `weight` its pre-shape strength;
    /// `attempt_failed` marks an input the world refused this frame.
    pub fn update(&mut self, observed: HandAffordance, weight: f32, attempt_failed: bool) {
        if attempt_failed {
            self.failed_frames = FAILED_PULSE_FRAMES;
        } else {
            self.failed_frames = self.failed_frames.saturating_sub(1);
        }

        if observed != HandAffordance::None {
            self.hover = observed;
            self.hover_weight = weight;
            self.hold_frames = HOVER_HOLD_FRAMES;
        } else if self.hold_frames > 0 {
            self.hold_frames -= 1;
        } else {
            self.hover = HandAffordance::None;
            self.hover_weight = 0.0;
        }
    }

    /// The resolved state: a refused attempt outranks the hover for its pulse,
    /// then the hover shows through again.
    pub fn state(&self) -> HandAffordance {
        if self.failed_frames > 0 {
            HandAffordance::Failed
        } else {
            self.hover
        }
    }

    /// How hard to pre-shape toward the hover's action. Read off the hover, not
    /// [`Self::state`]: the two channels are independent, so a failure pulse
    /// recolours the light without dropping the hand out of its shape.
    pub fn preshape(&self) -> HandPreshape {
        self.hover.preshape(self.hover_weight)
    }
}

/// The pre-shape strength for a target `distance` away, ramped between the
/// edge of reach and the fingertips.
pub fn preshape_weight(distance: f32) -> f32 {
    let t = (distance / crate::virtual_hand::FROB_REACH).clamp(0.0, 1.0);
    PRESHAPE_WEIGHT_NEAR + (PRESHAPE_WEIGHT_FAR - PRESHAPE_WEIGHT_NEAR) * t
}

/// Classify what a hand's ray hit, using the same predicates the hand's own
/// trigger/squeeze consult - never a second opinion about the target.
///
/// Order matters, and follows the input: the squeeze does not consult locks, so
/// a pickup it would take reads as a grab even if the object is somehow locked;
/// only a frob target's lock blocks it.
pub fn classify(world: &World, entity_id: EntityId) -> HandAffordance {
    let scripted = crate::virtual_hand::uses_scripted_world_frob(world, entity_id);
    if !scripted && crate::virtual_hand::can_grab_item(world, entity_id) {
        return HandAffordance::Grabbable;
    }
    if is_blocked(world, entity_id) {
        return HandAffordance::Blocked;
    }
    if scripted || is_frob_responsive(world, entity_id) {
        return HandAffordance::Frobbable;
    }
    HandAffordance::None
}

/// Whether the entity is locked against the player - the one lock predicate,
/// shared with buttons and door-opening AIs.
fn is_blocked(world: &World, entity_id: EntityId) -> bool {
    crate::scripts::script_util::is_entity_locked(world, entity_id)
}

/// Whether frobbing this entity plausibly does something: it hosts a world
/// panel's widget, authors a world frob action, or is a door (whose script
/// opens on Frob without authoring one).
///
/// A heuristic, deliberately: the trigger itself Frobs *whatever* the ray hit,
/// so the light cannot promise less than the input will attempt without
/// narrowing somewhere. This narrows it to what a frob can plausibly move, which
/// is stricter than flatscreen's highlight filter
/// (`flat_player_controller::is_frobbable`, any `PropFrobInfo` at all) - a wall
/// with an `IGNORE` action should not glow.
fn is_frob_responsive(world: &World, entity_id: EntityId) -> bool {
    use dark::properties::{FrobFlag, PropFrobInfo, PropTranslatingDoor};

    let is_panel_widget = world
        .borrow::<View<crate::gui::GuiPropProxyEntity>>()
        .map(|v| v.get(entity_id).is_ok())
        .unwrap_or(false);

    let authored = world
        .borrow::<View<PropFrobInfo>>()
        .map(|v| {
            v.get(entity_id).is_ok_and(|frob_info| {
                !frob_info.world_action.is_empty()
                    && !frob_info.world_action.contains(FrobFlag::IGNORE)
            })
        })
        .unwrap_or(false);

    is_panel_widget
        || authored
        || world
            .borrow::<View<PropTranslatingDoor>>()
            .map(|v| v.get(entity_id).is_ok())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hover that blinks out for a frame or two must not blink the light out
    /// with it - but a hover that is really gone must eventually drop to Off.
    #[test]
    fn hover_survives_a_short_dropout_then_expires() {
        let mut tracker = AffordanceTracker::default();
        tracker.update(HandAffordance::Grabbable, 0.3, false);
        assert_eq!(tracker.state(), HandAffordance::Grabbable);

        for _ in 0..HOVER_HOLD_FRAMES {
            tracker.update(HandAffordance::None, 0.0, false);
            assert_eq!(
                tracker.state(),
                HandAffordance::Grabbable,
                "hover should survive the hysteresis window"
            );
        }

        tracker.update(HandAffordance::None, 0.0, false);
        assert_eq!(tracker.state(), HandAffordance::None);
    }

    /// A refused attempt shows red for its pulse and then hands the light back
    /// to whatever the hand is still hovering.
    #[test]
    fn failed_pulse_lasts_its_window_then_restores_the_hover() {
        let mut tracker = AffordanceTracker::default();
        tracker.update(HandAffordance::Frobbable, 0.3, true);
        assert_eq!(tracker.state(), HandAffordance::Failed);

        for _ in 1..FAILED_PULSE_FRAMES {
            tracker.update(HandAffordance::Frobbable, 0.3, false);
            assert_eq!(tracker.state(), HandAffordance::Failed);
        }

        tracker.update(HandAffordance::Frobbable, 0.3, false);
        assert_eq!(tracker.state(), HandAffordance::Frobbable);
    }

    /// The failure pulse recolours the light without dropping the hand out of
    /// its shape - the two channels are independent.
    #[test]
    fn a_failure_pulse_keeps_the_hand_prompting() {
        let mut tracker = AffordanceTracker::default();
        tracker.update(HandAffordance::Grabbable, 0.3, true);

        assert_eq!(tracker.state(), HandAffordance::Failed);
        assert!(matches!(tracker.preshape(), HandPreshape::Grip(_)));
    }

    /// The two channels stay independent: eligibility colours the light, the
    /// action shapes the hand, and a state with no action prompts nothing.
    #[test]
    fn light_reads_eligibility_and_preshape_reads_the_action() {
        assert_eq!(HandAffordance::None.light(), HandLight::Off);
        assert_eq!(HandAffordance::Grabbable.light(), HandLight::Green);
        assert_eq!(HandAffordance::Frobbable.light(), HandLight::Green);
        assert_eq!(HandAffordance::Blocked.light(), HandLight::Amber);
        assert_eq!(HandAffordance::Failed.light(), HandLight::Red);

        assert!(matches!(
            HandAffordance::Grabbable.preshape(0.3),
            HandPreshape::Grip(_)
        ));
        assert!(matches!(
            HandAffordance::Frobbable.preshape(0.3),
            HandPreshape::Point(_)
        ));
        for state in [
            HandAffordance::None,
            HandAffordance::Blocked,
            HandAffordance::Failed,
        ] {
            assert_eq!(state.preshape(0.3), HandPreshape::None);
        }
    }

    /// The prompt ramps up as the hand closes in, and never reaches a full
    /// curl - the player's own trigger has to stay visible on top of it.
    #[test]
    fn preshape_weight_ramps_toward_the_target() {
        let far = preshape_weight(crate::virtual_hand::FROB_REACH);
        let near = preshape_weight(0.0);
        assert!(far < near, "{far} should prompt less than {near}");
        assert!((0.0..0.6).contains(&near), "{near} is too strong a prompt");
        assert!(preshape_weight(f32::MAX) >= PRESHAPE_WEIGHT_FAR);
    }
}
