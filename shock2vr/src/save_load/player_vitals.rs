use dark::properties::{PropHitPoints, PropMaxHitPoints, PropPsiState};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, ViewMut, World};

use crate::mission::PlayerInfo;

/// Restored pools reserve half the signed range for subsequent authored
/// healing/trait adjustments. Retail values are two digits; reaching this
/// boundary requires a malformed or hand-edited save.
const MAX_RESTORED_VITAL_POINTS: i32 = i32::MAX / 2;

/// One persisted current/maximum player resource pool.
///
/// Both values are signed in the save schema so malformed or hand-edited
/// saves can be normalized before the unsigned Dark `P$MAX_HP` property is
/// written back into the ECS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerVitalPool {
    pub current: i32,
    pub maximum: i32,
}

/// Capture the player identified by the mission's `PlayerInfo` unique.
pub(crate) fn capture_player_vitals(world: &World) -> Option<PlayerVitals> {
    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?;
    PlayerVitals::capture(world, player.entity_id)
}

/// Restore a persisted snapshot onto the destination mission's rebuilt player.
/// `None` intentionally leaves the template/career/trait-derived defaults in
/// place (legacy saves and first career selection).
pub(crate) fn restore_player_vitals(world: &World, vitals: Option<PlayerVitals>) {
    let Some(vitals) = vitals else {
        return;
    };
    let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
        return;
    };
    vitals.restore(world, player.entity_id);
}

impl PlayerVitalPool {
    fn clamped(self) -> PlayerVitalPool {
        let maximum = self.maximum.clamp(0, MAX_RESTORED_VITAL_POINTS);
        PlayerVitalPool {
            current: self.current.clamp(0, maximum),
            maximum,
        }
    }
}

/// Player health and psi state that follows the player between missions and is
/// stored independently from mission entities and held-item serialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerVitals {
    pub hit_points: PlayerVitalPool,
    pub psi_points: PlayerVitalPool,
}

impl PlayerVitals {
    /// Capture the live player pools. A scene without all four authored
    /// properties has no restorable vitals state.
    pub(crate) fn capture(world: &World, player: EntityId) -> Option<PlayerVitals> {
        let hit_points = world.borrow::<View<PropHitPoints>>().ok()?;
        let max_hit_points = world.borrow::<View<PropMaxHitPoints>>().ok()?;
        let psi_points = world.borrow::<View<PropPsiState>>().ok()?;
        let hp = hit_points.get(player).ok()?;
        let max_hp = max_hit_points.get(player).ok()?;
        let psi = psi_points.get(player).ok()?;

        Some(PlayerVitals {
            hit_points: PlayerVitalPool {
                current: hp.hit_points,
                maximum: i32::try_from(max_hp.hit_points).unwrap_or(i32::MAX),
            },
            psi_points: PlayerVitalPool {
                current: psi.psi_points,
                maximum: psi.max_psi_points,
            },
        })
    }

    /// Restore maximums first, then clamp each current value into its restored
    /// pool. Career and O/S-trait derivation runs before this method; a valid
    /// persisted snapshot therefore wins exactly, while malformed saves cannot
    /// create negative maximums or overfilled pools.
    pub(crate) fn restore(self, world: &World, player: EntityId) {
        let hit_points = self.hit_points.clamped();
        let psi_points = self.psi_points.clamped();

        world.run(
            |mut hp: ViewMut<PropHitPoints>,
             mut max_hp: ViewMut<PropMaxHitPoints>,
             mut psi: ViewMut<PropPsiState>| {
                if let Ok(value) = (&mut hp).get(player) {
                    value.hit_points = hit_points.current;
                }
                if let Ok(value) = (&mut max_hp).get(player) {
                    value.hit_points = hit_points.maximum as u32;
                }
                if let Ok(value) = (&mut psi).get(player) {
                    value.psi_points = psi_points.current;
                    value.max_psi_points = psi_points.maximum;
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_world() -> (World, EntityId) {
        let mut world = World::new();
        let player = world.add_entity((
            PropHitPoints { hit_points: 27 },
            PropMaxHitPoints { hit_points: 35 },
            PropPsiState {
                psi_points: 4,
                max_psi_points: 60,
                unknown: 50,
            },
        ));
        (world, player)
    }

    #[test]
    fn captures_current_and_maximum_pools_exactly() {
        let (world, player) = player_world();

        assert_eq!(
            PlayerVitals::capture(&world, player),
            Some(PlayerVitals {
                hit_points: PlayerVitalPool {
                    current: 27,
                    maximum: 35,
                },
                psi_points: PlayerVitalPool {
                    current: 4,
                    maximum: 60,
                },
            })
        );
    }

    #[test]
    fn restore_preserves_maximums_and_clamps_currents() {
        let (world, player) = player_world();
        PlayerVitals {
            hit_points: PlayerVitalPool {
                current: 80,
                maximum: 52,
            },
            psi_points: PlayerVitalPool {
                current: -3,
                maximum: 73,
            },
        }
        .restore(&world, player);

        let restored = PlayerVitals::capture(&world, player).unwrap();
        assert_eq!(
            restored,
            PlayerVitals {
                hit_points: PlayerVitalPool {
                    current: 52,
                    maximum: 52,
                },
                psi_points: PlayerVitalPool {
                    current: 0,
                    maximum: 73,
                },
            }
        );
        let psi = world.borrow::<View<PropPsiState>>().unwrap();
        assert_eq!(psi.get(player).unwrap().unknown, 50);
    }

    #[test]
    fn restore_limits_malformed_extreme_maxima_before_later_adjustments() {
        let (world, player) = player_world();
        PlayerVitals {
            hit_points: PlayerVitalPool {
                current: i32::MAX,
                maximum: i32::MAX,
            },
            psi_points: PlayerVitalPool {
                current: i32::MAX,
                maximum: i32::MAX,
            },
        }
        .restore(&world, player);

        let restored = PlayerVitals::capture(&world, player).unwrap();
        assert_eq!(
            restored.hit_points,
            PlayerVitalPool {
                current: MAX_RESTORED_VITAL_POINTS,
                maximum: MAX_RESTORED_VITAL_POINTS,
            }
        );
        assert_eq!(
            restored.psi_points,
            PlayerVitalPool {
                current: MAX_RESTORED_VITAL_POINTS,
                maximum: MAX_RESTORED_VITAL_POINTS,
            }
        );
    }
}
