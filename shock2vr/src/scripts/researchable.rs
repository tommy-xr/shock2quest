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
        // Only lift the research lock; a researched weapon saved Broken stays broken.
        if !crate::research::is_unresearched(world, entity_id) {
            return Effect::NoEffect;
        }
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

    #[test]
    fn completed_research_unlocks_only_unresearched_copies() {
        let mut quests = QuestInfo::new();
        let research = quests.research_mut();
        research.begin(-27, 1, 1);
        research.advance(-27, 60.0, 1, 1.0, 1.0, None, 0);
        assert!(research.is_complete(-27));
        let mut world = World::new();
        world.add_unique(quests);
        let copy = |state| (RuntimePropCanonicalTemplateId(-27), PropObjState(state));
        let locked = world.add_entity(copy(ObjectState::Unresearched));
        let broken = world.add_entity(copy(ObjectState::Broken));

        assert!(matches!(
            ResearchableScript::new().initialize(locked, &world),
            Effect::SetObjectState {
                state: ObjectState::Normal,
                ..
            }
        ));
        // A researched weapon saved Broken must not be repaired by a reload.
        assert!(matches!(
            ResearchableScript::new().initialize(broken, &world),
            Effect::NoEffect
        ));
    }
}
