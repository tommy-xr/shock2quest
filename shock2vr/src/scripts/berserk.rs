//! Adrenaline Overproduction (`Berserk`, template -3155): the tier-2
//! sustained buff that trades health for melee damage.
//!
//! The power's `PropPsiPower::data` is `[0.13, 1.0, 0, 0]`. **Assumption**
//! (nothing in the shipped data labels the floats): `data[0]` is the melee
//! damage bonus as a fraction, so a hit lands at `× (1 + 0.13)`, and `data[1]`
//! is the health the adrenaline costs the caster per second. That reading is
//! what makes the buff's authored numbers work out - +13% melee for 1 HP/s
//! over `10 × PSI` seconds - and matches the discipline's description (the
//! rush is self-harming).
//!
//! Like the other sustained powers, the hook lives beside the mechanic rather
//! than in the amp's cast path: the melee damage paths ask
//! [`melee_damage_multiplier`], and `MissionCore::update` ticks the drain
//! beside the radiation/healing ticks.

use dark::properties::PropHitPoints;
use shipyard::{Get, UniqueView, View, World};

use crate::mission::PlayerInfo;
use crate::psi::{ActivePsiPowers, BERSERK_TEMPLATE_ID, GlobalPsiPowers};

use super::Effect;

/// The floor the drain will not take the player below: Adrenaline
/// Overproduction hurts, but it must never be the thing that kills you.
const DRAIN_HP_FLOOR: i32 = 1;

/// The player's authored Berserk floats while the power is active.
fn active_data(world: &World) -> Option<[f32; 4]> {
    let active = world.borrow::<UniqueView<ActivePsiPowers>>().ok()?;
    if !active.is_active(BERSERK_TEMPLATE_ID) {
        return None;
    }
    let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
    powers
        .0
        .iter()
        .find(|power| power.template_id == BERSERK_TEMPLATE_ID)
        .map(|power| power.power.data)
}

/// What to scale a player melee hit by: `1 + data[0]` while Adrenaline
/// Overproduction is active, and 1 otherwise. Applied where the damage amount
/// is decided, so both the flat swing's raycast hit and a VR weapon's contact
/// damage get the bonus. Ranged damage never asks.
pub fn melee_damage_multiplier(world: &World) -> f32 {
    match active_data(world) {
        Some(data) => 1.0 + data[0],
        None => 1.0,
    }
}

/// Whole seconds of the power's duration elapsed during a frame, i.e. how many
/// 1-second drain ticks the frame owes. `remaining_secs` is the value *before*
/// `MissionCore` applies this frame's decrement (this tick runs first), so the
/// frame spans `remaining` down to `remaining - elapsed`.
fn drain_ticks(remaining_secs: f32, elapsed_secs: f32) -> i32 {
    let after = (remaining_secs - elapsed_secs).max(0.0);
    (remaining_secs.floor() - after.floor()).max(0.0) as i32
}

/// Drain the caster's health while Berserk is active - one `data[1]` HP per
/// second, floored at [`DRAIN_HP_FLOOR`] so the buff can never kill its own
/// caster. Called from `MissionCore::update` beside the healing/radiation
/// ticks, which keeps HP mutation at the mission's single effect choke point.
pub fn tick_player_drain(world: &World, elapsed_secs: f32) -> Option<Effect> {
    let data = active_data(world)?;
    let remaining = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .ok()?
        .0
        .iter()
        .find(|power| power.template_id == BERSERK_TEMPLATE_ID)?
        .remaining_secs;

    let drain = data[1] * drain_ticks(remaining, elapsed_secs) as f32;
    if drain <= 0.0 {
        return None;
    }

    let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?.entity_id;
    let current = world
        .borrow::<View<PropHitPoints>>()
        .ok()
        .and_then(|hit_points| hit_points.get(player).ok().map(|hp| hp.hit_points))?;
    let delta = -(drain.round() as i32).min(current - DRAIN_HP_FLOOR);
    (delta < 0).then_some(Effect::AdjustHitPoints {
        entity_id: player,
        delta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_owes_a_drain_tick_only_when_it_crosses_a_whole_second() {
        // 60Hz frames inside one second owe nothing...
        assert_eq!(drain_ticks(49.9, 1.0 / 60.0), 0);
        // ...the frame that crosses the boundary owes one...
        assert_eq!(drain_ticks(49.01, 1.0 / 60.0), 1);
        // ...and a long frame owes every second it swallowed.
        assert_eq!(drain_ticks(50.0, 3.0), 3);
    }

    #[test]
    fn a_frame_past_expiry_owes_no_more_than_the_power_had_left() {
        // The power ends this frame: the drain stops at 0, it does not keep
        // billing seconds the buff never ran for.
        assert_eq!(drain_ticks(2.5, 30.0), 2);
        assert_eq!(drain_ticks(0.5, 5.0), 0);
        assert_eq!(drain_ticks(0.0, 1.0), 0);
    }

    /// A world carrying the psi registry, the player, and `Berserk` active
    /// with `remaining` seconds left. `active: false` leaves it uncast.
    fn world_with_berserk(active: bool, remaining: f32, hit_points: i32) -> World {
        let mut world = World::new();
        let player = world.add_entity(PropHitPoints { hit_points });
        world.add_unique(PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: player,
        });
        world.add_unique(GlobalPsiPowers(vec![crate::psi::PsiPowerInfo {
            template_id: BERSERK_TEMPLATE_ID,
            name: "Berserk".to_owned(),
            display_name: None,
            power: dark::properties::PropPsiPower {
                power_id: 10,
                activation_type: crate::psi::ACTIVATION_TYPE_SUSTAINED,
                psi_cost: 2,
                // The authored floats: +13% melee, 1 HP/s.
                data: [0.13, 1.0, 0.0, 0.0],
            },
            projectiles: Vec::new(),
            overloadable: false,
            duration: None,
        }]));
        let mut powers = ActivePsiPowers::default();
        if active {
            powers.0.push(crate::psi::ActivePsiPower {
                template_id: BERSERK_TEMPLATE_ID,
                name: "Berserk".to_owned(),
                remaining_secs: remaining,
            });
        }
        world.add_unique(powers);
        world
    }

    #[test]
    fn an_active_berserk_scales_melee_by_its_authored_bonus() {
        let world = world_with_berserk(true, 50.0, 30);
        assert_eq!(melee_damage_multiplier(&world), 1.13);
        // ...and an uncast power leaves melee alone.
        let world = world_with_berserk(false, 0.0, 30);
        assert_eq!(melee_damage_multiplier(&world), 1.0);
    }

    #[test]
    fn the_drain_bills_one_hp_a_second_and_never_kills_the_caster() {
        let world = world_with_berserk(true, 49.01, 30);
        assert!(matches!(
            tick_player_drain(&world, 1.0 / 60.0),
            Some(Effect::AdjustHitPoints { delta: -1, .. })
        ));
        // A frame that crosses no second boundary bills nothing.
        let world = world_with_berserk(true, 49.9, 30);
        assert!(tick_player_drain(&world, 1.0 / 60.0).is_none());
        // At 1 HP the drain stops rather than finishing the player off.
        let world = world_with_berserk(true, 49.01, 1);
        assert!(tick_player_drain(&world, 1.0 / 60.0).is_none());
        // ...and a long frame at 2 HP takes only the one point it may.
        let world = world_with_berserk(true, 50.0, 2);
        assert!(matches!(
            tick_player_drain(&world, 3.0),
            Some(Effect::AdjustHitPoints { delta: -1, .. })
        ));
    }

    #[test]
    fn with_no_active_power_melee_damage_is_unscaled() {
        // No psi uniques at all (a unit-test world, or a scene without the
        // registry): the multiplier must be inert, not panic.
        let world = World::new();
        assert_eq!(melee_damage_multiplier(&world), 1.0);
        assert!(tick_player_drain(&world, 1.0).is_none());
    }
}
