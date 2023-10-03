use dark::properties::{InternalPropOriginalModelName, PropModelName, WrappedEntityId};
///
/// mission_entity_populator.rs
///
/// An implementation of EntityPopulator that creates entities based on the entity data
/// in a mission file
use shipyard::{Get, View, World};
use std::collections::HashMap;

use dark::mission::SystemShock2Level;
use dark::ss2_entity_info::SystemShock2EntityInfo;

use crate::mission::entity_creator;

use super::EntityPopulator;

pub struct MissionEntityPopulator {}

impl MissionEntityPopulator {
    pub fn create() -> MissionEntityPopulator {
        MissionEntityPopulator {}
    }
}

impl EntityPopulator for MissionEntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level: &SystemShock2Level,
        world: &mut World,
    ) -> HashMap<i32, WrappedEntityId> {
        let mut template_to_entity_id = HashMap::new();
        let mut all_entities = Vec::new();
        for (template_id, _props) in &level.entity_info.entity_to_properties {
            // Create the entity
            let entity = world.add_entity(());
            template_to_entity_id.insert(*template_id, WrappedEntityId(entity));

            all_entities.push((*template_id, entity))
        }

        // Second pass - hydrate properties
        for (template_id, _props) in &level.entity_info.entity_to_properties {
            let entity = template_to_entity_id.get(template_id).unwrap();
            entity_creator::initialize_entity_with_props(
                *template_id,
                gamesys_entity_info,
                world,
                entity.0,
                &level.obj_map,
            );

            // Augment any props

            let maybe_mod = {
                let maybe_model_name = world.borrow::<View<PropModelName>>().unwrap();
                if let Ok(model_name) = maybe_model_name.get(entity.0) {
                    Some(model_name.0.clone())
                } else {
                    None
                }
            };

            if let Some(model) = maybe_mod {
                world.add_component(entity.0, InternalPropOriginalModelName(model));
            }
        }

        // Third pass - initialize links for entity
        for (template_id, entity_id) in all_entities {
            entity_creator::initialize_links_for_entity(
                template_id,
                entity_id,
                gamesys_entity_info,
                &template_to_entity_id,
                world,
            );
        }

        template_to_entity_id
    }
}
