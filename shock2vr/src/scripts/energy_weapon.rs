use dark::properties::PropGunState;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, scripts::script_util::active_gun_setting};

use super::{Effect, MessagePayload, Script};

/// Recharges an energy weapon's internal charge to its authored capacity.
///
/// Recharging is deliberately monotonic: a full or over-capacity weapon is
/// left alone. The resulting [`Effect::RechargeAmmo`] changes only
/// [`PropGunState::ammo`], preserving the weapon's condition, setting,
/// modification, selected ammo, and inventory/held state. Capacity is applied
/// against the live state so duplicate messages in one tick stay idempotent.
pub struct EnergyWeapon;

impl EnergyWeapon {
    pub fn new() -> Self {
        Self
    }
}

impl Script for EnergyWeapon {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Recharge) {
            return Effect::NoEffect;
        }

        let v_gun_state = world.borrow::<View<PropGunState>>().unwrap();
        let (Some(gun_desc), Ok(gun_state)) = (
            active_gun_setting(world, entity_id),
            v_gun_state.get(entity_id),
        ) else {
            return Effect::NoEffect;
        };
        let capacity = gun_desc.clip.max(0);

        if gun_state.ammo >= capacity {
            Effect::NoEffect
        } else {
            Effect::RechargeAmmo {
                entity_id,
                capacity,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{GunSettingDesc, PropBaseGunDesc, PropGunState};
    use shipyard::{Get, View, ViewMut, World};

    use crate::{physics::PhysicsWorld, scripts::effect::recharge_ammo_to_capacity};

    use super::{Effect, EnergyWeapon, MessagePayload, Script};

    fn setting(clip: i32) -> GunSettingDesc {
        GunSettingDesc {
            clip,
            ..GunSettingDesc::default()
        }
    }

    fn gun_desc(clip: i32) -> PropBaseGunDesc {
        PropBaseGunDesc {
            settings: [setting(clip), setting(clip), setting(clip)],
        }
    }

    fn gun_state(ammo: i32) -> PropGunState {
        PropGunState {
            ammo,
            condition: 0.75,
            setting: 1,
            modification: 2,
            silence_value: 0.25,
        }
    }

    fn recharge(world: &World, weapon: shipyard::EntityId) -> Effect {
        EnergyWeapon::new().handle_message(
            weapon,
            world,
            &PhysicsWorld::new(),
            &MessagePayload::Recharge,
        )
    }

    #[test]
    fn recharge_refills_to_authored_capacity_without_mutating_other_state() {
        let mut world = World::new();
        let weapon = world.add_entity((gun_desc(100), gun_state(37)));

        match recharge(&world, weapon) {
            Effect::RechargeAmmo {
                entity_id,
                capacity,
            } => {
                assert_eq!(entity_id, weapon);
                assert_eq!(capacity, 100);
            }
            other => panic!("expected recharge ammo adjustment, got {other:?}"),
        }

        let states = world.borrow::<View<PropGunState>>().unwrap();
        let state = states.get(weapon).unwrap();
        assert_eq!(state.ammo, 37, "the effect pipeline owns ammo mutation");
        assert_eq!(state.condition, 0.75);
        assert_eq!(state.setting, 1);
        assert_eq!(state.modification, 2);
        assert_eq!(state.silence_value, 0.25);
    }

    #[test]
    fn recharge_uses_the_capacity_of_the_selected_fire_setting() {
        let mut world = World::new();
        // gun_state selects setting 1, whose capacity differs from setting 0's.
        let desc = PropBaseGunDesc {
            settings: [setting(50), setting(100), setting(0)],
        };
        let weapon = world.add_entity((desc, gun_state(0)));

        match recharge(&world, weapon) {
            Effect::RechargeAmmo { capacity, .. } => assert_eq!(capacity, 100),
            other => panic!("expected recharge ammo adjustment, got {other:?}"),
        }
    }

    #[test]
    fn recharge_at_capacity_is_idempotent() {
        let mut world = World::new();
        let weapon = world.add_entity((gun_desc(100), gun_state(100)));

        assert!(matches!(recharge(&world, weapon), Effect::NoEffect));
    }

    #[test]
    fn recharge_does_not_reduce_over_capacity_charge() {
        let mut world = World::new();
        let weapon = world.add_entity((gun_desc(100), gun_state(125)));

        assert!(matches!(recharge(&world, weapon), Effect::NoEffect));
    }

    #[test]
    fn duplicate_same_tick_recharges_apply_idempotently() {
        let mut world = World::new();
        let weapon = world.add_entity((gun_desc(100), gun_state(0)));
        // Script messages in one update all inspect the same pre-effect world.
        let effects = [recharge(&world, weapon), recharge(&world, weapon)];

        for effect in effects {
            let Effect::RechargeAmmo {
                entity_id,
                capacity,
            } = effect
            else {
                panic!("expected recharge effect");
            };
            let mut states = world.borrow::<ViewMut<PropGunState>>().unwrap();
            recharge_ammo_to_capacity((&mut states).get(entity_id).unwrap(), capacity);
        }

        let states = world.borrow::<View<PropGunState>>().unwrap();
        assert_eq!(states.get(weapon).unwrap().ammo, 100);
    }

    #[test]
    fn recharge_ignores_entities_without_gun_properties() {
        let mut world = World::new();
        let unrelated = world.add_entity(());

        assert!(matches!(recharge(&world, unrelated), Effect::NoEffect));
    }
}
