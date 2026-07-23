use std::{collections::HashMap, fs::File};

use dark::properties::{Links, WrappedEntityId};
use engine::game_log;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use crate::runtime_props::RuntimePropSelectedAmmo;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EntitySaveData {
    pub all_entities: Vec<u64>,
    pub template_id_to_entity_id: HashMap<i32, WrappedEntityId>,
    pub properties:
        HashMap<String /* prop name */, HashMap<u64 /*entity id*/, serde_json::Value>>,
    pub links: HashMap<u64 /*entity_id */, serde_json::Value>,
    /// Selected projectile-link index for weapons whose ammo type has been
    /// changed. Persisted separately because runtime components are not part of
    /// the Dark property registry.
    #[serde(default)]
    pub selected_ammo: HashMap<u64 /* entity id */, usize>,
}

impl EntitySaveData {
    pub fn empty() -> EntitySaveData {
        EntitySaveData {
            all_entities: Vec::new(),
            template_id_to_entity_id: HashMap::new(),
            properties: HashMap::new(),
            links: HashMap::new(),
            selected_ammo: HashMap::new(),
        }
    }
    pub fn instantiate(
        &self,
        world: &mut World,
    ) -> (HashMap<i32, WrappedEntityId>, HashMap<EntityId, EntityId>) {
        let original_template_to_entity_id = self.template_id_to_entity_id.clone();

        let mut old_entity_id_to_new_entity_id = HashMap::new();

        for entity_id_inner in self.all_entities.iter() {
            let new_entity = world.add_entity(());
            old_entity_id_to_new_entity_id
                .insert(EntityId::from_inner(*entity_id_inner).unwrap(), new_entity);
        }

        let mut template_to_entity_id = HashMap::new();
        for (template, ent) in original_template_to_entity_id {
            if let Some(new_entity) = old_entity_id_to_new_entity_id.get(&ent.0) {
                template_to_entity_id.insert(template, WrappedEntityId(*new_entity));
            }
        }

        let (all_properties, _, _) = dark::properties::get::<File>();

        for prop in all_properties {
            let name = prop.name();
            if let Some(prop_info) = self.properties.get(&name) {
                game_log!(DEBUG, "Deserializing property: {}", name);
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
        for (old_entity_id, selected_ammo) in &self.selected_ammo {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(*new_entity_id, RuntimePropSelectedAmmo(*selected_ammo));
            }
        }
        (template_to_entity_id, old_entity_id_to_new_entity_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::{Get, View};

    #[test]
    fn instantiate_restores_selected_ammo_on_the_remapped_entity() {
        let old_entity = EntityId::new_from_index_and_gen(7, 3);
        let mut data = EntitySaveData::empty();
        data.all_entities.push(old_entity.inner());
        data.selected_ammo.insert(old_entity.inner(), 2);
        let mut world = World::new();

        let (_, entity_map) = data.instantiate(&mut world);

        let new_entity = entity_map[&old_entity];
        let selected = world.borrow::<View<RuntimePropSelectedAmmo>>().unwrap();
        assert_eq!(selected.get(new_entity).unwrap().0, 2);
    }

    #[test]
    fn selected_ammo_defaults_empty_for_older_saves() {
        let data: EntitySaveData = serde_json::from_value(serde_json::json!({
            "all_entities": [],
            "template_id_to_entity_id": {},
            "properties": {},
            "links": {}
        }))
        .unwrap();

        assert!(data.selected_ammo.is_empty());
    }
}
