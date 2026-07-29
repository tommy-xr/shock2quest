use dark::properties::Link;
use engine::audio::AudioHandle;
use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{
    BaseButton, Effect, MessagePayload, Script,
    script_util::{get_first_link_of_type, play_environmental_sound, send_to_all_switch_links},
};

/// A moving-terrain call button whose `ScriptParams` link names the requested
/// station and whose `SwitchLink` names the elevator platform.
pub struct RerouteElevatorButton {
    base_button: BaseButton,
}

impl RerouteElevatorButton {
    pub fn new() -> Self {
        Self {
            base_button: BaseButton::new(),
        }
    }
}

impl Script for RerouteElevatorButton {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }
        if let Some(effect) = self.base_button.locked_effect(entity_id, world) {
            return effect;
        }
        let Some(target_waypoint) = get_first_link_of_type(world, entity_id, Link::ScriptParams)
        else {
            return Effect::NoEffect;
        };

        // Unlike BaseButton::activate_effect, this deliberately does not relay
        // TurnOn: authored reroute buttons replace MovingTerrain's TPathNext
        // with their ScriptParams target rather than advancing the path once.
        let reroute = send_to_all_switch_links(
            world,
            entity_id,
            MessagePayload::RerouteElevator { target_waypoint },
        );
        let sound =
            play_environmental_sound(world, entity_id, "activate", vec![], AudioHandle::new());
        Effect::combine(vec![reroute, sound])
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{Link, Links, ToLink, WrappedEntityId};

    use super::*;

    #[test]
    fn frob_sends_the_authored_waypoint_as_a_reroute_not_turn_on() {
        let mut world = World::new();
        let waypoint = world.add_entity(());
        let elevator = world.add_entity(());
        let button = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(waypoint)),
                    link: Link::ScriptParams,
                },
                ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(elevator)),
                    link: Link::SwitchLink,
                },
            ],
        });

        let effect = RerouteElevatorButton::new().handle_message(
            button,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        let effects = Effect::flatten(vec![effect]);

        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Send { msg }
                if msg.to == elevator
                    && matches!(
                        msg.payload,
                        MessagePayload::RerouteElevator { target_waypoint }
                            if target_waypoint == waypoint
                    )
        )));
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            Effect::Send { msg } if matches!(msg.payload, MessagePayload::TurnOn { .. })
        )));
    }
}
