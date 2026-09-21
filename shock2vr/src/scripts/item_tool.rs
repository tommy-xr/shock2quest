//! Item operations shared by the psi amp and the portable Recycler.
//!
//! Scripts name the source and target; the mission resolves the operation against
//! live state before applying its effects. This prevents two queued gestures from
//! spending the same points, recycling a deleted object, or losing a stack update.
use cgmath::InnerSpace;
use dark::properties::{
    PropAlchemy, PropEnergy, PropFabricate, PropFabricateCost, PropGunState, PropRecycle,
    PropStackCount, PropStackIncrement,
};
use shipyard::{EntityId, Get, UniqueView, View, World};

use super::{Effect, MessagePayload, Script, script_util};
use crate::{mission::PlayerInfo, physics::PhysicsWorld};

pub const ELECTRO_PSI: i32 = -3151;
pub const FABRICATE: i32 = -3149;
pub const ALCHEMY: i32 = -1147;

/// A captured cast, not a reservation of currency or an item. Revalidated when
/// an inventory target is chosen. None denotes the physical Recycler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemUse {
    pub tool: EntityId,
    pub power: Option<i32>,
    pub effective_psi: i32,
}

macro_rules! prop {
    ($world:expr, $entity:expr, $ty:ty) => {
        $world
            .borrow::<View<$ty>>()
            .ok()
            .and_then(|v| v.get($entity).ok().cloned())
    };
}

pub fn is_item_power(template: i32) -> bool {
    matches!(template, ELECTRO_PSI | FABRICATE | ALCHEMY)
}

pub fn is_recycler(world: &World, entity: EntityId) -> bool {
    script_util::entity_has_script(world, entity, "Recycler")
}

impl ItemUse {
    pub fn valid(self, world: &World) -> bool {
        if !script_util::player_carried_items(world).contains(&self.tool) {
            return false;
        }
        match self.power {
            None => is_recycler(world, self.tool),
            Some(id) => {
                crate::wielded_weapon::held_in_hand(world, self.tool)
                    && crate::wielded_weapon::is_psi_amp(world, self.tool)
                    && is_item_power(id)
                    && crate::psi_amp_selection::selected_power(world, self.tool)
                        .is_some_and(|p| p.template_id == id)
                    && super::psi_amp_script::power_is_known(world, id)
            }
        }
    }

    pub fn prompt(self) -> &'static str {
        match self.power {
            Some(ELECTRO_PSI) => "Select charge target (Tab cancels)",
            Some(FABRICATE) => "Select copy target (Tab cancels)",
            Some(ALCHEMY) => "Select alchemy target (Tab cancels)",
            _ => "Select recycle target (Tab cancels)",
        }
    }
}

/// In VR the tool hand's trigger explicitly operates on the other held item.
/// Flat enters the existing inventory canvas and waits for a target click.
pub fn begin(world: &World, request: ItemUse) -> Effect {
    if !request.valid(world) {
        return Effect::NoEffect;
    }
    if crate::mission::mission_core::presentation_is_vr(world) {
        let target = world.borrow::<UniqueView<PlayerInfo>>().ok().and_then(|p| {
            if p.left_hand_entity_id == Some(request.tool) {
                p.right_hand_entity_id
            } else if p.right_hand_entity_id == Some(request.tool) {
                p.left_hand_entity_id
            } else {
                None
            }
        });
        match target {
            Some(target) => Effect::ApplyItemUse { request, target },
            None => message("Hold the target item in your other hand."),
        }
    } else {
        Effect::BeginItemUse { request }
    }
}

pub struct Recycler;
impl Script for Recycler {
    fn handle_message(
        &mut self,
        entity: EntityId,
        world: &World,
        _: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::ProvideForConsumption { entity: target } => Effect::ApplyItemUse {
                request: ItemUse {
                    tool: entity,
                    power: None,
                    effective_psi: 0,
                },
                target: *target,
            },
            MessagePayload::Frob | MessagePayload::TriggerPull
                if crate::mission::mission_core::presentation_is_vr(world) =>
            {
                message("Hold the Recycler and release an item into it with your other hand.")
            }
            MessagePayload::Frob | MessagePayload::TriggerPull => begin(
                world,
                ItemUse {
                    tool: entity,
                    power: None,
                    effective_psi: 0,
                },
            ),
            _ => Effect::NoEffect,
        }
    }
}

pub fn message(text: impl Into<String>) -> Effect {
    Effect::ShowMessage { text: text.into() }
}

/// Resolve a single operation. `roll` is an integer in 1..=100; keeping the
/// random draw outside the policy makes success and failure independently testable.
/// The returned batch must run before any subsequent item operation is resolved.
pub fn resolve(world: &World, request: ItemUse, target: EntityId, roll: i32) -> Effect {
    if !request.valid(world)
        || target == request.tool
        || !(script_util::player_carried_items(world).contains(&target)
            || (request.power.is_none() && released_into_recycler(world, request.tool, target)))
    {
        return message("Select a different item you are carrying.");
    }
    let count = prop!(world, target, PropStackCount)
        .map(|p| p.0)
        .unwrap_or(1);
    if count <= 0 {
        return Effect::NoEffect;
    }
    let Some(power_id) = request.power else {
        let value = prop!(world, target, PropRecycle).map(|p| p.0).unwrap_or(0);
        let Some(amount) = value.checked_mul(count).filter(|v| *v > 0) else {
            return message("This item cannot be recycled.");
        };
        return Effect::combine(vec![
            Effect::DestroyEntity { entity_id: target },
            Effect::AwardNanites { amount },
            script_util::play_environmental_sound(
                world,
                request.tool,
                "activate",
                Vec::new(),
                engine::audio::AudioHandle::new(),
            ),
            message(format!("Recycled for {amount} nanites.")),
        ]);
    };
    let Some(power) = crate::psi_amp_selection::selected_power(world, request.tool) else {
        return Effect::NoEffect;
    };
    if super::psi_amp_script::player_psi_points(world) < power.power.psi_cost {
        return message("Not enough psi points.");
    }
    let data = power.power.data;
    let psi = request.effective_psi.max(0) as f32;
    let mut result = match power_id {
        ELECTRO_PSI => {
            let amount = data[0] * psi;
            if !amount.is_finite() || amount <= 0.0 {
                return Effect::NoEffect;
            }
            if crate::wielded_weapon::is_energy_weapon(world, target) {
                let Some(gun) = prop!(world, target, PropGunState) else {
                    return message("This item cannot be recharged.");
                };
                let Some(desc) = script_util::active_gun_setting(world, target) else {
                    return Effect::NoEffect;
                };
                let capacity = (desc.clip.max(0) as f32 * crate::implants::recharge_capacity(world)
                    / 100.0) as i32;
                let increment = (desc.clip.max(0) as f32 * amount / 100.0) as i32;
                let charged = gun.ammo.saturating_add(increment).min(capacity);
                if charged <= gun.ammo {
                    return message("This item is already fully charged.");
                }
                vec![
                    Effect::RechargeAmmo {
                        entity_id: target,
                        capacity: charged,
                    },
                    message("Item recharged."),
                ]
            } else if let Some(energy) = prop!(world, target, PropEnergy) {
                let increment = amount.min(crate::implants::recharge_capacity(world) - energy.0);
                if !energy.0.is_finite() || increment <= 0.0 {
                    return message("This item is already fully charged.");
                }
                vec![
                    Effect::RechargeItemEnergy {
                        entity_id: target,
                        level: energy.0 + increment,
                    },
                    message("Item recharged."),
                ]
            } else {
                return message("This item cannot be recharged.");
            }
        }
        FABRICATE => {
            let quantity = prop!(world, target, PropFabricate)
                .map(|p| p.0)
                .unwrap_or(0);
            let cost = prop!(world, target, PropFabricateCost)
                .map(|p| p.0)
                .unwrap_or(0);
            if quantity <= 0
                || cost < 0
                || prop!(world, target, PropStackCount).is_none()
                || count.checked_add(quantity).is_none()
            {
                return message("This item cannot be duplicated.");
            }
            let Some(payment) = script_util::spend_player_nanites(world, cost) else {
                return message(format!("Duplication requires {cost} nanites."));
            };
            let chance = fabrication_chance(data, request.effective_psi);
            if roll > chance {
                vec![message("Duplication failed. Try again.")]
            } else {
                vec![
                    payment,
                    Effect::AdjustStackCount {
                        entity_id: target,
                        delta: quantity,
                    },
                    message("Item duplicated."),
                ]
            }
        }
        ALCHEMY => {
            let value = prop!(world, target, PropAlchemy)
                .map(|p| p.0)
                .unwrap_or(0.0);
            let increment = prop!(world, target, PropStackIncrement)
                .map(|p| p.0)
                .unwrap_or(1)
                .max(1)
                .min(count);
            let amount = transmutation_yield(value, data[0], request.effective_psi, increment);
            if amount <= 0 {
                return message("This item cannot be transmuted.");
            }
            let consume = if increment == count {
                Effect::DestroyEntity { entity_id: target }
            } else {
                Effect::AdjustStackCount {
                    entity_id: target,
                    delta: -increment,
                }
            };
            vec![
                consume,
                Effect::AwardNanites { amount },
                message(format!("Transmuted for {amount} nanites.")),
            ]
        }
        _ => return Effect::NoEffect,
    };
    result.insert(
        0,
        Effect::SpendPsiPoints {
            amount: power.power.psi_cost,
        },
    );
    result.extend(super::psi_amp_script::amp_cast_flashes(world, request.tool));
    Effect::combine(result)
}

/// A fed item has just left its hand, so it is already a world object when the
/// queued offer runs. Accept it only beside a held Recycler; a distant ray hit
/// or a discarded item elsewhere must never be consumed.
fn released_into_recycler(world: &World, recycler: EntityId, target: EntityId) -> bool {
    if !crate::wielded_weapon::held_in_hand(world, recycler) {
        return false;
    }
    let Ok(transforms) = world.borrow::<View<crate::runtime_props::RuntimePropTransform>>() else {
        return false;
    };
    let (Ok(tool), Ok(item)) = (transforms.get(recycler), transforms.get(target)) else {
        return false;
    };
    (tool.0.w.truncate() - item.0.w.truncate()).magnitude2() <= 0.6 * 0.6
}

fn fabrication_chance(data: [f32; 4], psi: i32) -> i32 {
    (data[0] + data[1] * psi.max(0) as f32).clamp(0.0, 100.0) as i32
}

fn transmutation_yield(value: f32, multiplier: f32, psi: i32, count: i32) -> i32 {
    // Installed allobjs Alchemy::PsiTarget: one StackInc, value * Data1 *
    // (0.8 + 0.2 * PsiStat), truncated once after multiplication.
    let amount = value * multiplier * (0.8 + 0.2 * psi.max(0) as f32) * count as f32;
    if amount.is_finite() && amount > 0.0 && amount < i32::MAX as f32 {
        amount as i32
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mission::mission_core::{GlobalPresentationMode, GlobalTemplateClassTags},
        psi::{GlobalPsiPowers, PlayerPsiKnownPowers, PsiPowerInfo},
        psi_amp_selection::AmpSelection,
        quest_info::QuestInfo,
    };
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Links, PropPsiPower, PropPsiState, PropScripts, PropTemplateId};
    use std::collections::{HashMap, HashSet};

    fn fixture(power: Option<i32>) -> (World, ItemUse, EntityId) {
        let mut world = World::new();
        let player = world.add_entity(PropPsiState {
            psi_points: 20,
            max_psi_points: 20,
            unknown: 0,
        });
        let inventory = world.add_entity(Links { to_links: vec![] });
        let tool = world.add_entity((
            PropTemplateId { template_id: -247 },
            PropScripts {
                scripts: vec![
                    if power.is_some() {
                        "PsiAmpScript"
                    } else {
                        "Recycler"
                    }
                    .into(),
                ],
                inherits: false,
            },
            Links { to_links: vec![] },
        ));
        let target = world.add_entity((PropStackCount(12), Links { to_links: vec![] }));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: Some(target),
            right_hand_entity_id: Some(tool),
            inventory_entity_id: inventory,
        });
        world.add_unique(GlobalTemplateClassTags(HashMap::from([(
            -247,
            HashMap::from([("weapontype".into(), "psiamp".into())]),
        )])));
        let mut quest = QuestInfo::new();
        quest.player_stats_mut().award_nanites(100);
        quest.player_stats_mut().skills.maintenance = 2;
        world.add_unique(quest);
        if let Some(id) = power {
            world.add_component(
                tool,
                AmpSelection {
                    current: id,
                    alternate: None,
                },
            );
            world.add_unique(GlobalPsiPowers(vec![PsiPowerInfo {
                template_id: id,
                name: "test".into(),
                display_name: None,
                power: PropPsiPower {
                    power_id: 17,
                    activation_type: 4,
                    psi_cost: if id == ALCHEMY { 4 } else { 3 },
                    data: if id == FABRICATE {
                        [30.0, 10.0, 0.0, 0.0]
                    } else if id == ELECTRO_PSI {
                        [20.0, 0.0, 0.0, 0.0]
                    } else {
                        [1.0, 0.0, 0.0, 0.0]
                    },
                },
                projectiles: vec![],
                overloadable: true,
                duration: None,
            }]));
            world.add_unique(PlayerPsiKnownPowers(HashSet::from([id])));
        }
        (
            world,
            ItemUse {
                tool,
                power,
                effective_psi: 3,
            },
            target,
        )
    }
    fn effects(world: &World, request: ItemUse, target: EntityId, roll: i32) -> Vec<Effect> {
        Effect::flatten(vec![resolve(world, request, target, roll)])
    }
    fn no_payment(effects: &[Effect]) -> bool {
        !effects.iter().any(|e| {
            matches!(
                e,
                Effect::SpendPsiPoints { .. }
                    | Effect::SpendNanites { .. }
                    | Effect::AwardNanites { .. }
                    | Effect::DestroyEntity { .. }
                    | Effect::AdjustStackCount { .. }
            )
        })
    }

    #[test]
    fn vr_targets_the_opposite_hand_in_either_configuration() {
        let (world, request, target) = fixture(Some(ELECTRO_PSI));
        world.add_unique(GlobalPresentationMode(crate::PresentationMode::Vr));
        for swapped in [false, true] {
            if swapped {
                let mut p = world
                    .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
                    .unwrap();
                p.left_hand_entity_id = Some(request.tool);
                p.right_hand_entity_id = Some(target);
            }
            assert!(
                matches!(begin(&world, request), Effect::ApplyItemUse { target: t, .. } if t == target)
            );
        }
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .right_hand_entity_id = None;
        assert!(matches!(begin(&world, request), Effect::ShowMessage { .. }));
    }
    #[test]
    fn flat_begins_a_picker_without_spending_and_changed_power_invalidates_it() {
        let (mut world, request, _) = fixture(Some(ELECTRO_PSI));
        assert!(matches!(
            begin(&world, request),
            Effect::BeginItemUse { .. }
        ));
        world.add_component(
            request.tool,
            AmpSelection {
                current: ALCHEMY,
                alternate: None,
            },
        );
        assert!(!request.valid(&world));
    }
    #[test]
    fn recharge_is_scaled_clamped_and_preserves_the_target() {
        let (mut world, request, target) = fixture(Some(ELECTRO_PSI));
        world.add_component(target, PropEnergy(25.0));
        assert!(
            effects(&world, request, target, 1)
                .iter()
                .any(|e| matches!(e, Effect::RechargeItemEnergy {level, ..} if *level == 85.0))
        );
        world.add_component(target, PropEnergy(100.0));
        assert!(
            effects(&world, request, target, 1)
                .iter()
                .any(|e| matches!(e, Effect::RechargeItemEnergy {level, ..} if *level == 120.0))
        );
        world.add_component(target, PropEnergy(120.0));
        assert!(no_payment(&effects(&world, request, target, 1)));
    }
    #[test]
    fn fabrication_uses_authored_quantity_and_cost_and_failure_only_spends_psi() {
        let (mut world, request, target) = fixture(Some(FABRICATE));
        world.add_component(target, (PropFabricate(6), PropFabricateCost(45)));
        let success = effects(&world, request, target, 60);
        assert!(
            success
                .iter()
                .any(|e| matches!(e, Effect::SpendNanites { amount: 45 }))
        );
        assert!(
            success
                .iter()
                .any(|e| matches!(e, Effect::AdjustStackCount { delta: 6, .. }))
        );
        let failed = effects(&world, request, target, 61);
        assert!(
            failed
                .iter()
                .any(|e| matches!(e, Effect::SpendPsiPoints { amount: 3 }))
        );
        assert!(!failed.iter().any(|e| matches!(
            e,
            Effect::SpendNanites { .. } | Effect::AdjustStackCount { .. }
        )));
        world.add_component(target, PropFabricateCost(101));
        assert!(no_payment(&effects(&world, request, target, 1)));
    }
    #[test]
    fn alchemy_consumes_one_clip_while_recycler_consumes_the_whole_stack() {
        let (mut world, request, target) = fixture(Some(ALCHEMY));
        world.add_component(target, (PropAlchemy(1.25), PropStackIncrement(6)));
        let result = effects(&world, request, target, 1);
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::AdjustStackCount { delta: -6, .. }))
        );
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount: 10 }))
        );
        world.add_component(target, PropStackCount(2));
        let result = effects(&world, request, target, 1);
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity {entity_id} if *entity_id == target))
        );
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount: 3 }))
        );
        let (mut world, request, target) = fixture(None);
        world.add_component(target, PropRecycle(2));
        let result = effects(&world, request, target, 1);
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity {entity_id} if *entity_id == target))
        );
        assert!(
            result
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount: 24 }))
        );
        assert!(
            !result.iter().any(
                |e| matches!(e, Effect::DestroyEntity {entity_id} if *entity_id == request.tool)
            )
        );
    }
    #[test]
    fn ineligible_empty_unowned_and_self_targets_never_spend() {
        for power in [None, Some(ELECTRO_PSI), Some(FABRICATE), Some(ALCHEMY)] {
            let (mut world, request, target) = fixture(power);
            assert!(no_payment(&effects(&world, request, target, 1)));
            assert!(no_payment(&effects(&world, request, request.tool, 1)));
            world.add_component(
                target,
                (
                    PropEnergy(0.0),
                    PropFabricate(1),
                    PropFabricateCost(1),
                    PropAlchemy(1.0),
                    PropRecycle(1),
                    PropStackCount(0),
                ),
            );
            assert!(no_payment(&effects(&world, request, target, 1)));
            world.add_component(target, PropStackCount(1));
            world
                .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
                .unwrap()
                .left_hand_entity_id = None;
            assert!(no_payment(&effects(&world, request, target, 1)));
        }
    }
    #[test]
    fn malformed_values_and_overflow_cannot_mint_nanites_or_wrap_a_stack() {
        assert_eq!(transmutation_yield(f32::NAN, 1.0, 6, 1), 0);
        assert_eq!(transmutation_yield(f32::INFINITY, 1.0, 6, 1), 0);
        let (mut world, request, target) = fixture(Some(FABRICATE));
        world.add_component(target, (PropStackCount(i32::MAX), PropFabricate(1)));
        assert!(no_payment(&effects(&world, request, target, 1)));
        assert_eq!(fabrication_chance([30.0, 10.0, 0.0, 0.0], 8), 100);
    }
}
