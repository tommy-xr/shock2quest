use shipyard::{EntityId, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::get_all_switch_links};

pub struct TrapSlayer {}
impl TrapSlayer {
    pub fn new() -> TrapSlayer {
        TrapSlayer {}
    }
}
impl Script for TrapSlayer {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                let mut effs = Vec::new();
                let destroy_self = vec![Effect::DestroyEntity { entity_id }];
                let slay_linked_ents: Vec<Effect> = get_all_switch_links(world, entity_id)
                    .iter()
                    .map(|e| Effect::SlayEntity { entity_id: *e })
                    .collect();
                effs.extend(destroy_self);
                effs.extend(slay_linked_ents);

                Effect::Combined { effects: effs }
            }
            _ => Effect::NoEffect,
        }
    }
}
