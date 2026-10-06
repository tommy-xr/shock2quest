//! Shared inventory-tool decisions for cursor drags and close VR gestures.
//! Application is queued through one effect so each use is revalidated after
//! earlier uses have finished (a second device cannot repair the same gun).
use dark::properties::{ObjectState, PropGunState, PropRecycle, PropRepairDiff, PropStackCount};
use shipyard::{EntitiesView, EntityId, Get, View, World};

use crate::scripts::{Effect, MessagePayload, Script, maintenance, script_util};

pub fn is_tool(world: &World, tool: EntityId) -> bool {
    maintenance::is_maintenance_tool(world, tool)
        || script_util::entity_has_script(world, tool, "FreeRepair")
        || script_util::entity_has_script(world, tool, "Recycler")
}

pub fn offers_to(world: &World, tool: EntityId, target: EntityId) -> bool {
    if maintenance::is_maintenance_tool(world, tool) {
        return maintenance::offers_to(world, tool, target);
    }
    if script_util::entity_has_script(world, tool, "Recycler") {
        return tool != target
            && world
                .borrow::<View<PropRecycle>>()
                .is_ok_and(|v| v.get(target).is_ok());
    }
    script_util::entity_has_script(world, tool, "FreeRepair")
        && tool != target
        && (world
            .borrow::<View<PropGunState>>()
            .is_ok_and(|v| v.get(target).is_ok())
            || world
                .borrow::<View<PropRepairDiff>>()
                .is_ok_and(|v| v.get(target).is_ok()))
}

fn repair_refusal(world: &World, target: Option<EntityId>) -> Option<&'static str> {
    let Some(target) = target else {
        return Some("Apply the auto-repair device to a broken item.");
    };
    if crate::scripts::gui::object_state(world, target) != ObjectState::Broken {
        return Some("The item is not broken. Device kept.");
    }
    if crate::wielded_weapon::is_psi_amp(world, target) {
        return Some("This item cannot be repaired. Device kept.");
    }
    None
}

pub fn can_apply(world: &World, tool: EntityId, target: EntityId) -> bool {
    if script_util::entity_has_script(world, tool, "Recycler") {
        return tool != target && recycle_value(world, target).is_ok();
    }
    if maintenance::is_maintenance_tool(world, tool) {
        return matches!(
            maintenance::maintenance_outcome(world, Some(target)),
            maintenance::MaintenanceOutcome::Restored { .. }
        );
    }
    offers_to(world, tool, target) && repair_refusal(world, Some(target)).is_none()
}

pub fn preview(world: &World, tool: EntityId, target: EntityId) -> String {
    if script_util::entity_has_script(world, tool, "Recycler") {
        return match recycle_value(world, target) {
            Ok(value) => format!("Recycle entire item stack for {value} nanites"),
            Err(reason) => reason.into(),
        };
    }
    if maintenance::is_maintenance_tool(world, tool) {
        return maintenance::preview(world, target);
    }
    repair_refusal(world, Some(target))
        .unwrap_or("Repair item; consumes 1 auto-repair device")
        .to_owned()
}

pub fn apply(world: &World, tool: EntityId, target: Option<EntityId>) -> Effect {
    if !world
        .borrow::<EntitiesView>()
        .is_ok_and(|e| e.is_alive(tool))
        || world
            .borrow::<View<PropStackCount>>()
            .is_ok_and(|v| v.get(tool).is_ok_and(|s| s.0 <= 0))
    {
        return Effect::NoEffect;
    }
    if maintenance::is_maintenance_tool(world, tool) {
        return maintenance::apply(world, tool, target);
    }
    if script_util::entity_has_script(world, tool, "Recycler") {
        let value = target
            .filter(|id| *id != tool)
            .ok_or("Apply the recycler to an item to convert it to nanites.")
            .and_then(|id| recycle_value(world, id));
        return match value {
            Ok(amount) => Effect::combine(vec![
                Effect::DestroyEntity {
                    entity_id: target.unwrap(),
                },
                Effect::AwardNanites { amount },
                Effect::ShowMessage {
                    text: format!("Recycled for {amount} nanites."),
                },
            ]),
            Err(reason) => Effect::ShowMessage {
                text: reason.into(),
            },
        };
    }
    if !script_util::entity_has_script(world, tool, "FreeRepair") {
        return Effect::NoEffect;
    }
    if let Some(reason) = repair_refusal(world, target) {
        return Effect::ShowMessage {
            text: reason.into(),
        };
    }
    let target = target.unwrap();
    if !offers_to(world, tool, target) {
        return Effect::ShowMessage {
            text: "This item cannot be repaired. Device kept.".into(),
        };
    }
    Effect::combine(vec![
        crate::weapon_repair::success(target, world),
        maintenance::consume_tool(world, tool),
    ])
}

fn recycle_value(world: &World, target: EntityId) -> Result<i32, &'static str> {
    let value = world
        .borrow::<View<PropRecycle>>()
        .ok()
        .and_then(|v| v.get(target).ok().map(|p| p.0))
        .unwrap_or(0);
    let count = world
        .borrow::<View<PropStackCount>>()
        .ok()
        .and_then(|v| v.get(target).ok().map(|p| p.0))
        .unwrap_or(1);
    if value <= 0 || count <= 0 {
        return Err("This item cannot be recycled.");
    }
    Ok(value.saturating_mul(count))
}

/// Inventory activation uses the held gun; explicit drags/VR contact name their
/// own target and do not silently choose a different hand.
pub struct ItemTool;
impl Script for ItemTool {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &crate::physics::PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if matches!(msg, MessagePayload::Frob) {
            Effect::ApplyItemTool {
                tool: entity_id,
                // Recyclers require an explicit target: activating the tool
                // alone must never destroy the currently wielded weapon.
                target: script_util::entity_has_script(world, entity_id, "FreeRepair")
                    .then(|| crate::wielded_weapon::wielded_weapon(world))
                    .flatten(),
            }
        } else {
            Effect::NoEffect
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{PropObjState, PropScripts};

    #[test]
    fn recycler_uses_authored_stack_value_keeps_device_and_refuses_unpriced_items() {
        let mut world = World::new();
        let tool = world.add_entity((PropScripts {
            scripts: vec!["Recycler".into()],
            inherits: false,
        },));
        let item = world.add_entity((PropRecycle(3), PropStackCount(6)));
        let effects = Effect::flatten(vec![apply(&world, tool, Some(item))]);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::AwardNanites { amount: 18 }))
        );
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == item))
        );
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == tool))
        );
        assert!(matches!(
            apply(&world, tool, Some(tool)),
            Effect::ShowMessage { .. }
        ));
        let unpriced = world.add_entity(());
        assert!(matches!(
            apply(&world, tool, Some(unpriced)),
            Effect::ShowMessage { .. }
        ));
        world.delete_entity(item);
        assert!(matches!(
            apply(&world, tool, Some(item)),
            Effect::ShowMessage { .. }
        ));
    }

    #[test]
    fn auto_repair_bypasses_skill_consumes_one_and_refuses_healthy_items() {
        let mut world = World::new();
        let gun = world.add_entity((
            PropObjState(ObjectState::Broken),
            PropGunState {
                ammo: 7,
                condition: 0.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
        ));
        let tool = world.add_entity((
            PropScripts {
                scripts: vec!["FreeRepair".into()],
                inherits: false,
            },
            PropStackCount(2),
        ));
        assert!(can_apply(&world, tool, gun));
        let effects = Effect::flatten(vec![apply(&world, tool, Some(gun))]);
        assert!(effects.iter().any(|e| matches!(e, Effect::SetObjectState { entity_id, state: ObjectState::Normal } if *entity_id == gun)));
        assert!(effects.iter().any(
            |e| matches!(e, Effect::AdjustStackCount { entity_id, delta: -1 } if *entity_id == tool)
        ));
        // Once the first effect batch repairs the item, a repeated use refuses.
        world.add_component(gun, PropObjState(ObjectState::Normal));
        assert!(!can_apply(&world, tool, gun));
        assert!(matches!(
            apply(&world, tool, Some(gun)),
            Effect::ShowMessage { .. }
        ));
        world.add_component(tool, PropStackCount(0));
        assert!(matches!(apply(&world, tool, Some(gun)), Effect::NoEffect));
    }
}
