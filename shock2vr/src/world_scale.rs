//! Physical size of the world. Personal equipment keeps its authored physical size.
use std::sync::atomic::{AtomicU32, Ordering};

static ACTIVE: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldScale(f32);

impl WorldScale {
    pub const DEFAULT: Self = Self(1.0);

    pub fn active() -> Self {
        Self(f32::from_bits(ACTIVE.load(Ordering::Relaxed)))
    }

    pub fn requested() -> Self {
        Self(crate::dev_params::get(crate::dev_params::WORLD_SCALE))
    }

    pub fn factor(self) -> f32 {
        self.0
    }

    pub fn meters_per_world_unit(self) -> f32 {
        crate::METERS_PER_WORLD_UNIT * self.0
    }

    /// Convert baseline personal-space dimensions into the scaled world's units.
    pub fn personal(self) -> f32 {
        1.0 / self.0
    }

    // Commit only after the player's new collision shape fits. UI edits may
    // arrive while paused or mid-frame; tracking/rendering use the accepted scale.
    pub(crate) fn activate(self) {
        ACTIVE.store(self.0.to_bits(), Ordering::Relaxed);
    }
}

pub fn meters_per_world_unit() -> f32 {
    WorldScale::active().meters_per_world_unit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_shrinks_while_personal_equipment_keeps_its_physical_size() {
        let hybrid_world_height = 2.2385707 / crate::METERS_PER_WORLD_UNIT;
        let glove_world_length = 0.20 / crate::METERS_PER_WORLD_UNIT;
        let weapon_world_length = 0.65 / crate::METERS_PER_WORLD_UNIT;
        let small = WorldScale(0.82);
        let hybrid_m = hybrid_world_height * small.meters_per_world_unit();
        assert!((hybrid_m - 1.835628).abs() < 0.00001);
        for (authored, physical) in [(glove_world_length, 0.20), (weapon_world_length, 0.65)] {
            assert!(
                (authored * small.personal() * small.meters_per_world_unit() - physical).abs()
                    < 0.00001
            );
        }
    }
}
