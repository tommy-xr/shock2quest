//! A support hand must be attached when a VR wrench swing starts and stay
//! attached through contact. Uses the current interaction attachment, never
//! controller proximity or the weapon body's collision-recovery velocity.
use shipyard::{EntityId, Unique, UniqueView};

/// One hand's swing state. `weapon` is what the hand is holding, so a swing
/// cannot survive putting the weapon down and picking another one up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SwingLatch {
    weapon: Option<EntityId>,
    hot: bool,
    two_handed: bool,
    sound: SwingSound,
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

    pub fn sound_ready(&mut self, speed: Option<f32>, dt: f32) -> bool {
        self.sound.update(speed, dt)
    }

    #[cfg(test)]
    pub fn hot(&self) -> bool {
        self.hot
    }

    #[cfg(test)]
    pub fn two_handed(&self) -> bool {
        self.two_handed
    }
}

/// A whoosh marks crossing the damage-speed gate, once until the hand slows.
/// The lower rearm point keeps tracking noise around the gate from chattering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SwingSound {
    armed: bool,
    cooldown: f32,
}

impl SwingSound {
    fn update(&mut self, speed: Option<f32>, dt: f32) -> bool {
        let Some(speed) = speed.filter(|s| s.is_finite()) else {
            *self = Self::default();
            return false;
        };
        if dt <= 0.0 || !dt.is_finite() {
            return false;
        }
        self.cooldown = (self.cooldown - dt).max(0.0);
        let threshold = crate::dev_params::get(crate::dev_params::MELEE_FREE_SWING_SPEED);
        if speed < threshold * 0.7 {
            self.armed = true;
        }
        if self.armed && self.cooldown == 0.0 && speed >= threshold {
            self.armed = false;
            self.cooldown = 0.25;
            return true;
        }
        false
    }
}

/// Both flat attacks and physical VR swings use the weapon's motion schema.
/// The shipped psi sword has none, so it falls back to the shard's whoosh.
pub(crate) fn sound_effect(world: &shipyard::World, entity: EntityId) -> crate::scripts::Effect {
    use crate::scripts::{Effect, script_util::play_environmental_sound};
    use dark::{EnvSoundQuery, properties::PropLimbModel};
    use engine::audio::AudioHandle;
    use shipyard::{Get, View};
    if !world
        .borrow::<View<PropLimbModel>>()
        .is_ok_and(|models| models.get(entity).is_ok())
    {
        return Effect::NoEffect;
    }
    let effect = play_environmental_sound(
        world,
        entity,
        "motion",
        vec![("plyrmelee", "2"), ("plyrmeleeswing", "1")],
        AudioHandle::new(),
    );
    match effect {
        Effect::PlayEnvironmentalSound {
            audio_handle,
            query,
            position,
        } => Effect::PlayEnvironmentalSoundWithFallback {
            audio_handle,
            query,
            position,
            fallback: EnvSoundQuery::from_tag_values(vec![
                ("event", "motion"),
                ("weapontype", "crystalshard"),
                ("plyrmelee", "2"),
                ("plyrmeleeswing", "1"),
            ]),
        },
        other => other,
    }
}

#[derive(Clone, Copy, Debug, Default, Unique)]
pub(crate) struct MeleeSwings(pub [SwingLatch; 2]);

impl MeleeSwings {
    /// Current-frame releases can downgrade the physics step's latch; a new
    /// grab cannot upgrade that already-started swing.
    pub fn release_unsupported(&mut self, supported: impl Fn(EntityId) -> bool) {
        for latch in &mut self.0 {
            latch.two_handed &= latch.weapon.is_some_and(&supported);
        }
    }
}

pub(crate) fn damage_scale(world: &shipyard::World, weapon: EntityId) -> f32 {
    use shipyard::{Get, View};
    let eligible = world
        .borrow::<View<dark::properties::PropModelName>>()
        .ok()
        .and_then(|models| {
            models.get(weapon).ok().map(|model| {
                // Two-handed melee weapons, once they have a support socket.
                // The psi sword is one-handed by design.
                matches!(model.0.as_str(), "wrench_h" | "rapier_h" | "shard_h")
                    && crate::vr_support::supports_model(&model.0)
            })
        })
        .unwrap_or(false);
    if !eligible {
        return 1.0;
    }
    let supported = world
        .borrow::<UniqueView<MeleeSwings>>()
        .is_ok_and(|swings| {
            swings
                .0
                .iter()
                .any(|latch| latch.weapon == Some(weapon) && latch.two_handed)
        });
    if supported {
        1.0
    } else {
        crate::dev_params::get(crate::dev_params::MELEE_ONE_HAND_SCALE)
    }
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

#[cfg(test)]
mod sound_tests {
    use super::SwingSound;

    #[test]
    fn whoosh_marks_damage_threshold_once_until_slow_again() {
        let mut gate = SwingSound::default();
        let threshold = crate::dev_params::get(crate::dev_params::MELEE_FREE_SWING_SPEED);
        assert!(!gate.update(Some(0.0), 0.016));
        assert!(!gate.update(Some(threshold * 0.9), 0.016));
        assert!(gate.update(Some(threshold), 0.016));
        for _ in 0..100 {
            assert!(!gate.update(Some(threshold * 1.1), 0.016));
            assert!(!gate.update(Some(threshold * 0.9), 0.016));
        }
        assert!(!gate.update(Some(0.0), 0.3));
        assert!(gate.update(Some(threshold), 0.016));
    }

    #[test]
    fn pickup_and_tracking_recovery_require_a_slow_sample() {
        let mut gate = SwingSound::default();
        assert!(!gate.update(Some(10.0), 0.016));
        assert!(!gate.update(Some(0.0), 0.016));
        assert!(!gate.update(None, 0.016));
        assert!(!gate.update(Some(10.0), 0.016));
        assert!(!gate.update(Some(0.0), 0.016));
        assert!(!gate.update(Some(10.0), 0.0));
        assert!(gate.update(Some(10.0), 0.016));
        assert!(!gate.update(Some(0.0), 0.016));
        assert!(!gate.update(Some(10.0), 0.016));
    }
}
