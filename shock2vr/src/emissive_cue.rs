//! Item state read off the glow of a held item's authored emissive parts
//! (the 25AE models give them their own material slot), e.g. the psi amp's
//! strip dims as psi points drain. Only scales emissivity the art already
//! has, so models without a glowing part are unaffected.
use dark::properties::PropPsiState;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{dev_params, mission::PlayerInfo, time::Time};

/// Glow left at a sliver of charge, so "low" still reads as "on".
const FLOOR: f32 = 0.1;
/// Below this fraction the glow stutters, warning of an imminent empty.
const LOW: f32 = 0.15;

/// Emissivity scale for a held item, or `None` to keep the authored glow.
pub(crate) fn held_scale(world: &World, item: EntityId) -> Option<f32> {
    if !dev_params::get_bool(dev_params::EMISSIVE_CUES)
        || !crate::wielded_weapon::is_psi_amp(world, item)
    {
        return None;
    }
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?.entity_id;
    let psi = world.borrow::<View<PropPsiState>>().ok()?;
    let psi = psi.get(player).ok()?;
    let fraction = psi.psi_points as f32 / psi.max_psi_points.max(1) as f32;
    let secs = world.borrow::<UniqueView<Time>>().ok()?.total.as_secs_f32();
    Some(glow(fraction, secs))
}

/// Quadratic above a floor - the glow saturates on top of the lit colour, so
/// linear left half charge looking full. Stutters when low, dark at empty.
fn glow(fraction: f32, secs: f32) -> f32 {
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return 0.0;
    }
    let level = FLOOR + (1.0 - FLOOR) * fraction * fraction;
    // Two incommensurate sines give an irregular, deterministic stutter.
    let dropout = (secs * 23.0).sin() + (secs * 9.1).sin() > 1.2;
    if fraction < LOW && dropout {
        level * 0.3
    } else {
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_dark_and_full_is_authored() {
        assert_eq!(glow(0.0, 0.0), 0.0);
        assert_eq!(glow(1.0, 0.0), 1.0);
    }

    #[test]
    fn brightness_rises_with_charge_above_the_floor() {
        assert!(glow(0.5, 0.0) > glow(0.25, 0.0));
        assert!(glow(0.01, 0.0) >= FLOOR);
    }

    #[test]
    fn only_low_charge_stutters() {
        let dims =
            |fraction| (0..600).any(|i| glow(fraction, i as f32 / 60.0) < glow(fraction, 0.0));
        assert!(dims(0.1));
        assert!(!dims(0.5));
    }
}
