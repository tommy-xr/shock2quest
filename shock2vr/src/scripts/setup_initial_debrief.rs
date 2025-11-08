use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, Message, MessagePayload, Script, script_util};

pub struct SetupInitialDebriefScript {}

impl SetupInitialDebriefScript {
    pub fn new() -> SetupInitialDebriefScript {
        SetupInitialDebriefScript {}
    }
}

impl Script for SetupInitialDebriefScript {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                println!("SetupInitialDebrief: Setting up debrief system");

                // Since this entity has no switch links, we need to manually trigger
                // the specific Inverter entity that controls DEBRIEF1-DOOR
                // From analysis, we know DEBRIEF1-DOOR is controlled by an Inverter

                // First, try to find DEBRIEF1-DOOR entity
                let debrief_door_entities =
                    script_util::get_entities_by_name(world, "DEBRIEF1-DOOR");

                if let Some(door_entity) = debrief_door_entities.first() {
                    println!(
                        "SetupInitialDebrief: Found DEBRIEF1-DOOR entity {:?}",
                        door_entity
                    );

                    // Send TurnOn directly to the door
                    Effect::Send {
                        msg: Message {
                            payload: MessagePayload::TurnOn { from: entity_id },
                            to: *door_entity,
                        },
                    }
                } else {
                    println!("SetupInitialDebrief: DEBRIEF1-DOOR entity not found!");
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
