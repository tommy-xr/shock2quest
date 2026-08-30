use std::collections::HashMap;
use std::time::Duration;

use shipyard::{EntityId, Unique};

/// How long a damaged creature keeps showing its brackets and health bar with
/// nothing highlighting it. The original marks the object with a `HUDTime`
/// expiry when it takes damage, which is why a creature you shoot from cover
/// stays legible for a moment afterwards.
pub const DAMAGE_FLASH_DURATION: Duration = Duration::from_secs(5);

/// Entities whose HUD overlay is currently forced on by recent damage, keyed by
/// the elapsed mission time at which the overlay stops.
#[derive(Unique, Default)]
pub struct DamageFlash {
    expiry: HashMap<EntityId, Duration>,
}

impl DamageFlash {
    pub fn arm(&mut self, entity_id: EntityId, now: Duration) {
        self.expiry.insert(entity_id, now + DAMAGE_FLASH_DURATION);
    }

    /// The entities still flashing at `now`. Expired entries are dropped here
    /// rather than on a timer, so the map cannot grow with every creature the
    /// player has ever shot.
    pub fn active(&mut self, now: Duration) -> Vec<EntityId> {
        self.expiry.retain(|_, expiry| *expiry > now);
        self.expiry.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::World;

    fn entity() -> EntityId {
        World::new().add_entity(())
    }

    #[test]
    fn damage_shows_the_overlay_for_five_seconds() {
        let (mut flash, id) = (DamageFlash::default(), entity());
        flash.arm(id, Duration::from_secs(10));

        assert_eq!(flash.active(Duration::from_secs(14)), vec![id]);
        assert!(flash.active(Duration::from_secs(15)).is_empty());
    }

    #[test]
    fn a_second_hit_restarts_the_window() {
        let (mut flash, id) = (DamageFlash::default(), entity());
        flash.arm(id, Duration::from_secs(10));
        flash.arm(id, Duration::from_secs(14));

        assert_eq!(flash.active(Duration::from_secs(18)), vec![id]);
    }
}
