//! Shared inventory-tool decisions for cursor drags and close VR gestures.
//! Application is queued through one effect so each use is revalidated after
//! earlier uses have finished (a second device cannot repair the same gun).
use dark::properties::{ObjectState, PropGunState, PropRepairDiff, PropStackCount};
use shipyard::{EntitiesView, EntityId, Get, View, World};

use crate::scripts::{Effect, MessagePayload, Script, maintenance, script_util};

pub fn is_tool(world: &World, tool: EntityId) -> bool {
    maintenance::is_maintenance_tool(world, tool)
        || script_util::entity_has_script(world, tool, "FreeRepair")
}

pub fn offers_to(world: &World, tool: EntityId, target: EntityId) -> bool {
    if maintenance::is_maintenance_tool(world, tool) {
        return maintenance::offers_to(world, tool, target);
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
    if maintenance::is_maintenance_tool(world, tool) {
        return matches!(
            maintenance::maintenance_outcome(world, Some(target)),
            maintenance::MaintenanceOutcome::Restored { .. }
        );
    }
    offers_to(world, tool, target) && repair_refusal(world, Some(target)).is_none()
}

pub fn preview(world: &World, tool: EntityId, target: EntityId) -> String {
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
                target: crate::wielded_weapon::wielded_weapon(world),
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
