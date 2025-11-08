use shipyard::{EntityId, World};
use tracing::info;

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::get_all_switch_links};

pub struct TrapDestroyer {}
impl TrapDestroyer {
    pub fn new() -> TrapDestroyer {
        TrapDestroyer {}
    }
}
impl Script for TrapDestroyer {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                info!("destroying!");

                let mut effs = vec![Effect::DestroyEntity { entity_id }];
                let switchlink_effs: Vec<Effect> = get_all_switch_links(world, entity_id)
                    .iter()
                    .map(|e| Effect::DestroyEntity { entity_id: *e })
                    .collect();

                effs.extend(switchlink_effs);

                Effect::Combined { effects: effs }
            }
            _ => Effect::NoEffect,
        }
    }
}
