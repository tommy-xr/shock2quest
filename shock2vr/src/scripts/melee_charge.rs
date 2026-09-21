//! Smasher's held-trigger windup. Transient, like gun bursts and psi charges:
//! dropping a weapon or loading a save cancels it without banking an attack.
use shipyard::{EntityId, UniqueView, World};

use super::Effect;

const CHARGE_SECONDS: f32 = 0.380;
pub(super) const BONUS_DAMAGE: f32 = 6.0;
// A VR release opens one physical swing, rather than storing damage forever.
const STRIKE_WINDOW_SECONDS: f32 = 0.8;

pub(super) fn owned(world: &World) -> bool {
    world
        .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
        .is_ok_and(|q| q.player_stats().has_os_trait(super::gui::TRAIT_SMASHER))
}

#[derive(Default)]
pub(super) struct MeleeCharge {
    elapsed: Option<f32>,
    strike_remaining: f32,
}

impl MeleeCharge {
    pub(super) fn begin(&mut self, entity_id: EntityId) -> Effect {
        // Repeated press messages cannot restart a charge or bank two strikes.
        self.elapsed.get_or_insert(0.0);
        self.strike_remaining = 0.0;
        self.readout(entity_id)
    }

    pub(super) fn charging(&self) -> bool {
        self.elapsed.is_some()
    }

    pub(super) fn bonus(&self) -> f32 {
        if self.strike_remaining > 0.0 {
            BONUS_DAMAGE
        } else {
            0.0
        }
    }

    pub(super) fn tick(&mut self, entity_id: EntityId, dt: f32, held: bool) -> Effect {
        if !held {
            return self.cancel(entity_id);
        }
        if let Some(elapsed) = &mut self.elapsed {
            *elapsed += dt;
            return self.readout(entity_id);
        }
        if self.strike_remaining > 0.0 {
            self.strike_remaining = (self.strike_remaining - dt).max(0.0);
            if self.strike_remaining == 0.0 {
                return self.clear(entity_id);
            }
        }
        Effect::NoEffect
    }

    pub(super) fn release(&mut self, entity_id: EntityId, vr: bool) -> Option<Effect> {
        let charged = self.elapsed.take()? >= CHARGE_SECONDS;
        if vr {
            self.strike_remaining = if charged { STRIKE_WINDOW_SECONDS } else { 0.0 };
            Some(if charged {
                self.readout(entity_id)
            } else {
                self.clear(entity_id)
            })
        } else {
            Some(Effect::combine(vec![
                self.clear(entity_id),
                Effect::FlatMeleeSwing {
                    entity_id,
                    bonus_damage: if charged { BONUS_DAMAGE } else { 0.0 },
                },
            ]))
        }
    }

    pub(super) fn landed(&mut self, entity_id: EntityId) -> Effect {
        if self.strike_remaining == 0.0 {
            return Effect::NoEffect;
        }
        self.strike_remaining = 0.0;
        self.clear(entity_id)
    }

    pub(super) fn cancel(&mut self, entity_id: EntityId) -> Effect {
        let active = self.elapsed.take().is_some() || self.strike_remaining > 0.0;
        self.strike_remaining = 0.0;
        if active {
            self.clear(entity_id)
        } else {
            Effect::NoEffect
        }
    }

    fn clear(&self, entity_id: EntityId) -> Effect {
        Effect::SetMeleeCharge {
            entity_id,
            fraction: None,
        }
    }

    fn readout(&self, entity_id: EntityId) -> Effect {
        Effect::SetMeleeCharge {
            entity_id,
            fraction: Some(
                self.elapsed
                    .map(|s| (s / CHARGE_SECONDS).min(1.0))
                    .unwrap_or(1.0),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn threshold_short_release_expiry_and_consumption() {
        let id = EntityId::dead();
        let mut charge = MeleeCharge::default();
        charge.begin(id);
        charge.tick(id, 0.379, true);
        charge.release(id, true);
        assert_eq!(charge.bonus(), 0.0);
        charge.begin(id);
        charge.tick(id, 0.381, true);
        charge.release(id, true);
        assert_eq!(charge.bonus(), 6.0);
        charge.landed(id);
        assert_eq!(charge.bonus(), 0.0);
        charge.begin(id);
        charge.tick(id, 1.0, true);
        charge.release(id, true);
        charge.tick(id, 0.81, true);
        assert_eq!(charge.bonus(), 0.0);
    }
    #[test]
    fn putting_away_cancels_charge_and_released_strike() {
        let id = EntityId::dead();
        let mut charge = MeleeCharge::default();
        for release in [false, true] {
            charge.begin(id);
            charge.tick(id, 1.0, true);
            if release {
                charge.release(id, true);
            }
            charge.tick(id, 0.0, false);
            assert!(!charge.charging());
            assert_eq!(charge.bonus(), 0.0);
            assert!(charge.release(id, true).is_none());
        }
    }
}
