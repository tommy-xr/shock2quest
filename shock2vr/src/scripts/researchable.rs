use dark::properties::ObjectState;
use shipyard::{EntityId, World};

use crate::{physics::PhysicsWorld, quest_info::QuestInfo};

use super::{Effect, MessagePayload, Script, script_util::entity_class_template_id};

/// Keeps newly-created copies of a campaign-researched archetype usable. The
/// Research MFD itself handles frobs; this script only reconciles persistent
/// campaign state onto the live Dark object state during initialization.
pub struct ResearchableScript;

impl ResearchableScript {
    pub fn new() -> Self {
        Self
    }
}

impl Script for ResearchableScript {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let Some(template_id) = entity_class_template_id(world, entity_id) else {
            return Effect::NoEffect;
        };
        let researched = world
            .borrow::<shipyard::UniqueView<QuestInfo>>()
            .map(|quests| quests.research().is_complete(template_id))
            .unwrap_or(false);
        if researched {
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Normal,
            }
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _msg: &MessagePayload,
    ) -> Effect {
        Effect::NoEffect
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{ObjectState, PropObjState};

    use crate::runtime_props::RuntimePropCanonicalTemplateId;

    use super::*;

    #[test]
    fn incomplete_research_preserves_authored_object_state() {
        let mut world = World::new();
        let entity_id = world.add_entity((
            RuntimePropCanonicalTemplateId(-1341),
            PropObjState(ObjectState::Normal),
        ));
        world.add_unique(QuestInfo::new());

        let effect = ResearchableScript::new().initialize(entity_id, &world);

        assert!(matches!(effect, Effect::NoEffect));
    }
}
