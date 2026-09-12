use super::{Effect, MessagePayload, Script};
use crate::{mission::PlayerInfo, physics::PhysicsWorld};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

pub enum HazardObject {
    Room,
    ToxinPatch,
    Cleanse(bool),
    Environment,
    EngineCleanup,
    Armor,
}
impl Script for HazardObject {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match self {
            Self::Room => {
                let (with, entered) = match msg {
                    MessagePayload::SensorBeginIntersect { with } => (*with, true),
                    MessagePayload::SensorEndIntersect { with } => (*with, false),
                    _ => return Effect::NoEffect,
                };
                if world
                    .borrow::<UniqueView<PlayerInfo>>()
                    .is_ok_and(|p| p.entity_id == with)
                {
                    Effect::RadiationRoom { entity_id, entered }
                } else {
                    Effect::NoEffect
                }
            }
            Self::ToxinPatch if matches!(msg, MessagePayload::Frob) => {
                let pharma = world
                    .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
                    .is_ok_and(|q| {
                        q.player_stats()
                            .has_os_trait(crate::scripts::gui::TRAIT_PHARMO_FRIENDLY)
                    });
                Effect::UseToxinPatch {
                    entity_id,
                    amount: if pharma { 2.4 } else { 2.0 },
                }
            }
            Self::Cleanse(toxin) if matches!(msg, MessagePayload::TurnOn { .. }) => {
                Effect::ClearHazard { toxin: *toxin }
            }
            Self::EngineCleanup if matches!(msg, MessagePayload::TurnOn { .. }) => {
                // allobjs 0x10039620: SlayAllByName(ConsumeType), ClearRadiation,
                // EngineRadClear=1, unlock EngineCoreDoor, then optional UseMsg.
                let mut effects = vec![
                    Effect::ClearEnvironmentalRadiation,
                    Effect::SetQuestBit {
                        quest_bit_name: "EngineRadClear".to_owned(),
                        quest_bit_value: dark::properties::QuestBitValue::INCOMPLETE,
                    },
                ];
                if let Ok((values, metadata, hierarchy, templates)) = world.borrow::<(
                    View<dark::properties::PropConsumeType>,
                    UniqueView<crate::mission::mission_core::GlobalEntityMetadata>,
                    UniqueView<crate::mission::mission_core::GlobalTemplateHierarchy>,
                    View<dark::properties::PropTemplateId>,
                )>() {
                    if let Ok(name) = values.get(entity_id) {
                        if let Some(class) = metadata.0.get(&name.0.to_ascii_lowercase()) {
                            for (id, _) in (&templates).iter().with_id() {
                                if super::script_util::entity_class_template_id(world, id)
                                    .is_some_and(|t| {
                                        hierarchy.is_or_descends_from(t, class.template_id)
                                    })
                                {
                                    effects.push(Effect::SlayEntity { entity_id: id });
                                }
                            }
                        }
                    }
                }
                effects.extend(
                    super::script_util::get_entities_by_name(world, "enginecoredoor")
                        .into_iter()
                        .map(|entity_id| Effect::SetLocked {
                            entity_id,
                            locked: false,
                        }),
                );
                effects.push(
                    super::trap_message::TrapMessage.handle_message(entity_id, world, physics, msg),
                );
                Effect::combine(effects)
            }
            Self::Environment if matches!(msg, MessagePayload::TurnOn { .. }) => {
                Effect::ClearEnvironmentalRadiation
            }
            Self::Armor if matches!(msg, MessagePayload::Frob) => {
                Effect::ToggleHazardArmor { entity_id }
            }
            _ => Effect::NoEffect,
        }
    }
}
