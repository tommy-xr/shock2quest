use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script};

/// The station's `SecurityComputer` console - "hack to temporarily disable
/// security". Using it stands the station alarm down: the alarm's count is
/// cleared and every alerted ecology is reset, which returns it to the normal
/// population profile and (over its switch links) clears the cameras that
/// raised the alarm.
///
/// The console has no links of its own; the stand-down reaches the ecologies
/// by scanning for their alert state (`security_alarm`).
///
/// Retail gates this behind the console's hack attempt. This port has no
/// hacking minigame for it yet, so using the console *is* the success path;
/// the stand-down itself is what a successful hack would trigger.
pub struct SecurityComputer;

impl SecurityComputer {
    pub fn new() -> Self {
        Self
    }
}

impl Script for SecurityComputer {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::Frob) {
            return Effect::NoEffect;
        }
        Effect::ClearSecurityAlarm { from: entity_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console() -> (World, EntityId) {
        let mut world = World::new();
        let console = world.add_entity(());
        (world, console)
    }

    #[test]
    fn using_the_console_stands_security_down() {
        let (world, console) = console();
        let effect = SecurityComputer::new().handle_message(
            console,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::Frob,
        );
        assert!(matches!(
            effect,
            Effect::ClearSecurityAlarm { from } if from == console
        ));
    }

    #[test]
    fn other_messages_are_ignored() {
        let (world, console) = console();
        let effect = SecurityComputer::new().handle_message(
            console,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::TurnOn { from: console },
        );
        assert!(matches!(effect, Effect::NoEffect));
    }
}
