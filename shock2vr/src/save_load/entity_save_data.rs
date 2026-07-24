use std::{collections::HashMap, fs::File};

use dark::properties::{Links, WrappedEntityId};
use engine::game_log;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use crate::runtime_props::RuntimePropDeathPose;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EntitySaveData {
    pub all_entities: Vec<u64>,
    pub template_id_to_entity_id: HashMap<i32, WrappedEntityId>,
    pub properties:
        HashMap<String /* prop name */, HashMap<u64 /*entity id*/, serde_json::Value>>,
    pub links: HashMap<u64 /*entity_id */, serde_json::Value>,
    /// Resolved generated death motions keyed by the pre-save entity ID.
    ///
    /// This is separate from authored `P$CretPose` data so mission-placed
    /// corpse decorations retain their existing semantics. Older saves omit
    /// the field and load with no generated terminal poses.
    #[serde(default)]
    pub death_poses: HashMap<u64, RuntimePropDeathPose>,
}

impl EntitySaveData {
    pub fn empty() -> EntitySaveData {
        EntitySaveData {
            all_entities: Vec::new(),
            template_id_to_entity_id: HashMap::new(),
            properties: HashMap::new(),
            links: HashMap::new(),
            death_poses: HashMap::new(),
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

        for (old_entity_id, death_pose) in &self.death_poses {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(*new_entity_id, death_pose.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::{Get, View};

    #[test]
    fn legacy_save_without_death_poses_defaults_to_empty() {
        let save: EntitySaveData = serde_json::from_str(
            r#"{
                "all_entities": [],
                "template_id_to_entity_id": {},
                "properties": {},
                "links": {}
            }"#,
        )
        .unwrap();

        assert!(save.death_poses.is_empty());
    }

    #[test]
    fn instantiate_remaps_death_pose_to_the_new_entity_id() {
        let mut source_world = World::new();
        let old_entity = source_world.add_entity(());
        let mut save = EntitySaveData::empty();
        save.all_entities.push(old_entity.inner());
        save.death_poses.insert(
            old_entity.inner(),
            RuntimePropDeathPose("resolved_death".to_owned()),
        );

        let mut loaded_world = World::new();
        let _sentinel = loaded_world.add_entity(());
        let (_, old_to_new) = save.instantiate(&mut loaded_world);
        let new_entity = old_to_new[&old_entity];

        assert_ne!(new_entity, old_entity);
        let poses = loaded_world.borrow::<View<RuntimePropDeathPose>>().unwrap();
        assert_eq!(poses.get(new_entity).unwrap().0, "resolved_death");
        assert!(poses.get(old_entity).is_err());
    }
}
