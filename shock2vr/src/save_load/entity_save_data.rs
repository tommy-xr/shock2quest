use std::fs::File;
use rustc_hash::FxHashMap;

use dark::properties::{Links, WrappedEntityId};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, IntoIter, World};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EntitySaveData {
    pub all_entities: Vec<u64>,
    pub template_id_to_entity_id: FxHashMap<i32, WrappedEntityId>,
    pub properties:
        FxHashMap<String /* prop name */, FxHashMap<u64 /*entity id*/, serde_json::Value>>,
    pub links: FxHashMap<u64 /*entity_id */, serde_json::Value>,
}

impl EntitySaveData {
    pub fn empty() -> EntitySaveData {
        EntitySaveData {
            all_entities: Vec::new(),
            template_id_to_entity_id: FxHashMap::default(),
            properties: FxHashMap::default(),
            links: FxHashMap::default(),
        }
    }
    pub fn instantiate(
        &self,
        world: &mut World,
    ) -> (FxHashMap<i32, WrappedEntityId>, FxHashMap<EntityId, EntityId>) {
        let original_template_to_entity_id = self.template_id_to_entity_id.clone();

        let mut old_entity_id_to_new_entity_id = FxHashMap::default();

        for entity_id_inner in self.all_entities.iter() {
            let new_entity = world.add_entity(());
            old_entity_id_to_new_entity_id
                .insert(EntityId::from_inner(*entity_id_inner).unwrap(), new_entity);
        }

        let mut template_to_entity_id = FxHashMap::default();
        for (template, ent) in original_template_to_entity_id {
            if let Some(new_entity) = old_entity_id_to_new_entity_id.get(&ent.0) {
                template_to_entity_id.insert(template, WrappedEntityId(*new_entity));
            }
        }

        let (all_properties, _, _) = dark::properties::get::<File>();

        for prop in all_properties {
            let name = prop.name();
            if let Some(prop_info) = self.properties.get(&name) {
                println!("deserializing: {}", name);
                prop.deserialize(prop_info, world, &old_entity_id_to_new_entity_id);
            }
        }

        // Now, we need to hydrate the links

        for (old_entity_id, link) in &self.links {
            let entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&entity_id) {
                let links = Links::deserialize(link.clone(), &old_entity_id_to_new_entity_id);
                world.add_component(*new_entity_id, links);
            }
        }
        (template_to_entity_id, old_entity_id_to_new_entity_id)
    }
}
