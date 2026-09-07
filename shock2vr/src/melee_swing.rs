//! When a VR melee swing goes *hot*, and whether it was two-handed at the
//! moment it did.
//!
//! A swing is hot while the weapon's head is travelling faster than
//! [`crate::dev_params::MELEE_FREE_SWING_SPEED`] - the same gate that decides
//! whether a contact bills damage, read here a moment earlier and without a
//! contact to project onto. Two-handedness is sampled once, on the rising
//! edge, and can only ever be *lost* afterwards:
//!
//! - a second hand taken **after** the swing was already moving does not
//!   upgrade it - a grab a frame before contact is not a two-handed swing;
//! - a second hand released **before** contact downgrades it - the blow landed
//!   with one hand on the weapon whatever the swing started as.
//!
//! Below the gate there is no swing to latch and the answer is just "are both
//! hands on it now" - a contact can bill on the *victim's* speed (a creature
//! charging onto a held blade), and that blow is two-handed if the weapon is.
//!
//! Eligibility comes from [`crate::two_hand_grip`]'s latch (a resolved support
//! attachment), never from how close the other controller happens to be.

use shipyard::{EntityId, Unique, UniqueView};

use crate::vr_config::{Handedness, hand_slot};

/// One hand's swing state. `weapon` is what the hand is holding, so a swing
/// cannot survive putting the weapon down and picking another one up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SwingLatch {
    weapon: Option<EntityId>,
    hot: bool,
    two_handed: bool,
}

impl SwingLatch {
    /// Fold one frame in: what the hand holds, whether its head is over the
    /// swing gate, and whether a support hand is on it right now.
    pub fn update(&mut self, weapon: Option<EntityId>, hot: bool, supported: bool) {
        if weapon != self.weapon {
            *self = Self::default();
            self.weapon = weapon;
        }
        match (hot, self.hot) {
            // Rising edge: this is the one frame two-handedness is sampled.
            (true, false) => self.two_handed = supported,
            // Still swinging: losing the support hand loses the latch, and
            // gaining one cannot raise it.
            (true, true) => self.two_handed &= supported,
            // Below the gate there is no swing to latch, so the answer is
            // simply whether both hands are on the weapon *now*. A contact can
            // still bill while the weapon is barely moving - a creature
            // charging onto a held blade impales itself on its own speed - and
            // that blow is two-handed if the player is holding it in two.
            (false, _) => self.two_handed = supported,
        }
        self.hot = hot;
    }

    pub fn hot(&self) -> bool {
        self.hot
    }

    pub fn two_handed(&self) -> bool {
        self.two_handed
    }

    pub fn weapon(&self) -> Option<EntityId> {
        self.weapon
    }
}

/// The per-hand latches, indexed by [`hand_slot`]. Published into the world so
/// the (pure) melee script can read this frame's answer at contact.
#[derive(Clone, Copy, Debug, Default, Unique)]
pub struct MeleeSwings([SwingLatch; 2]);

impl MeleeSwings {
    pub fn set(&mut self, hand: Handedness, latch: SwingLatch) {
        self.0[hand_slot(hand)] = latch;
    }

    pub fn get(&self, hand: Handedness) -> SwingLatch {
        self.0[hand_slot(hand)]
    }

    /// The latch on `weapon`, if either hand is swinging it.
    pub fn of(&self, weapon: EntityId) -> Option<SwingLatch> {
        self.0
            .iter()
            .find(|latch| latch.weapon == Some(weapon))
            .copied()
    }

    /// The swing worth reporting: a hot one if there is one, else whichever
    /// hand holds a melee weapon.
    pub fn reportable(&self) -> Option<SwingLatch> {
        self.0
            .iter()
            .find(|latch| latch.hot)
            .or_else(|| self.0.iter().find(|latch| latch.weapon.is_some()))
            .copied()
    }
}

/// Whether the swing carrying `weapon` was latched two-handed. `false` on any
/// presentation that never publishes the latches (flat), which is also the
/// answer that costs a two-handed weapon its one-hand penalty.
pub fn latched_two_handed(world: &shipyard::World, weapon: EntityId) -> bool {
    world
        .borrow::<UniqueView<MeleeSwings>>()
        .ok()
        .and_then(|swings| swings.of(weapon))
        .is_some_and(|latch| latch.two_handed)
}

/// How fast a held melee weapon's *head* is travelling relative to the player,
/// in world units per second - the swing gate's reading when there is no
/// contact to project onto.
///
/// The head, not the centre of mass: a weapon swung about the wrist moves its
/// far end fast while its centre barely moves, so a centre reading under-reads
/// exactly the gesture the gate is meant to catch. Read off the hand's drive
/// target rather than the weapon body, for the same reason the melee script's
/// own gate reads it - an obstructed weapon reports its catch-up as speed the
/// player never produced.
pub fn swing_speed(physics: &crate::physics::PhysicsWorld, weapon: EntityId) -> Option<f32> {
    let velocity = physics.held_melee_head_velocity(weapon)?;
    Some(crate::physics::relative_swing_speed(
        velocity,
        physics.player_velocity(),
        cgmath::Vector3::new(0.0, 0.0, 0.0),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weapon() -> Option<EntityId> {
        EntityId::from_inner(1)
    }

    /// The baseline: a swing that starts two-handed and stays that way keeps
    /// its latch.
    #[test]
    fn a_swing_started_in_two_hands_stays_two_handed() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), false, true);
        latch.update(weapon(), true, true);
        assert!(latch.hot() && latch.two_handed());
    }

    /// The cheat this exists to stop: grabbing the weapon with the second hand
    /// after the swing is already moving must not upgrade it.
    #[test]
    fn a_second_hand_taken_after_the_swing_went_hot_does_not_upgrade_it() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), true, false);
        latch.update(weapon(), true, true);
        assert!(!latch.two_handed());
    }

    /// And the converse: letting go before the blow lands downgrades it.
    #[test]
    fn releasing_the_second_hand_before_impact_downgrades_the_swing() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), true, true);
        assert!(latch.two_handed());
        latch.update(weapon(), true, false);
        assert!(!latch.two_handed());
    }

    /// Dropping below the gate ends the swing, so the next one samples afresh.
    #[test]
    fn a_swing_that_ends_stops_being_hot() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), true, true);
        latch.update(weapon(), false, true);
        assert!(!latch.hot());
        latch.update(weapon(), true, false);
        assert!(!latch.two_handed(), "the next swing samples afresh");
    }

    /// A blow that lands while the weapon is barely moving - a creature
    /// charging onto a held blade - is still two-handed if the player is
    /// holding it in two hands.
    #[test]
    fn a_weapon_held_still_in_two_hands_is_two_handed() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), false, true);
        assert!(!latch.hot() && latch.two_handed());
        latch.update(weapon(), false, false);
        assert!(!latch.two_handed());
    }

    /// A latch belongs to the weapon it was taken on; swapping weapons in the
    /// same hand must not carry a hot two-handed swing over to the new one.
    #[test]
    fn changing_weapons_resets_the_latch() {
        let mut latch = SwingLatch::default();
        latch.update(weapon(), true, true);
        latch.update(EntityId::from_inner(2), true, false);
        assert!(!latch.two_handed());
    }
}
