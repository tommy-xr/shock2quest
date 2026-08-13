use std::{collections::HashMap, fs::File};

use dark::properties::{Links, WrappedEntityId};
use engine::game_log;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use crate::runtime_props::{
    RuntimePropCanonicalTemplateId, RuntimePropDeathPose, RuntimePropLaunchedProjectile,
    RuntimePropSelectedAmmo,
};
use crate::scripts::SavedScriptState;

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
    /// Selected projectile-link index for weapons whose ammo type has been
    /// changed. Persisted separately because runtime components are not part of
    /// the Dark property registry.
    #[serde(default)]
    pub selected_ammo: HashMap<u64 /* entity id */, usize>,
    /// Stable gamesys archetype for entities whose `PropTemplateId` is a
    /// positive, mission-local object ID.
    #[serde(default)]
    pub canonical_template_ids: HashMap<u64 /* entity id */, i32>,
    /// Entities created through Dark's `launchProjectile` path. The marker
    /// restores dynamic authored physics and keeps empty animation players
    /// from taking velocity ownership after load.
    #[serde(default)]
    pub launched_projectiles: Vec<u64 /* entity id */>,
    /// Opt-in private state owned by scripts on these entities. Registered ECS
    /// properties and links remain in their existing fields above; this is only
    /// for runtime modes, timers, latches, and similar script internals.
    #[serde(default)]
    pub script_states: Vec<SavedScriptState>,
}

impl EntitySaveData {
    pub fn empty() -> EntitySaveData {
        EntitySaveData {
            all_entities: Vec::new(),
            template_id_to_entity_id: HashMap::new(),
            properties: HashMap::new(),
            links: HashMap::new(),
            death_poses: HashMap::new(),
            selected_ammo: HashMap::new(),
            canonical_template_ids: HashMap::new(),
            launched_projectiles: Vec::new(),
            script_states: Vec::new(),
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
        for (old_entity_id, selected_ammo) in &self.selected_ammo {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(*new_entity_id, RuntimePropSelectedAmmo(*selected_ammo));
            }
        }
        for (old_entity_id, canonical_template_id) in &self.canonical_template_ids {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(
                    *new_entity_id,
                    RuntimePropCanonicalTemplateId(*canonical_template_id),
                );
            }
        }
        for old_entity_id in &self.launched_projectiles {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(*new_entity_id, RuntimePropLaunchedProjectile);
            }
        }
        (template_to_entity_id, old_entity_id_to_new_entity_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::{SavedScriptState, ScriptState, ScriptStateIdentity};
    use dark::properties::{Link, ToLink};
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
            RuntimePropDeathPose::new("resolved_death".to_owned(), Some(-1.6)),
        );

        let mut loaded_world = World::new();
        let _sentinel = loaded_world.add_entity(());
        let (_, old_to_new) = save.instantiate(&mut loaded_world);
        let new_entity = old_to_new[&old_entity];

        assert_ne!(new_entity, old_entity);
        let poses = loaded_world.borrow::<View<RuntimePropDeathPose>>().unwrap();
        assert_eq!(
            poses.get(new_entity).unwrap(),
            &RuntimePropDeathPose::new("resolved_death".to_owned(), Some(-1.6))
        );
        assert!(poses.get(old_entity).is_err());
    }

    #[test]
    fn legacy_death_pose_string_loads_without_a_floor_depth() {
        let pose: RuntimePropDeathPose = serde_json::from_str(r#""resolved_death""#).unwrap();

        assert_eq!(
            pose,
            RuntimePropDeathPose::new("resolved_death".to_owned(), None)
        );
    }

    #[test]
    fn death_pose_round_trip_preserves_the_live_floor_depth() {
        let pose = RuntimePropDeathPose::new("resolved_death".to_owned(), Some(-1.6));

        let encoded = serde_json::to_string(&pose).unwrap();
        let decoded: RuntimePropDeathPose = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, pose);
    }

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
        assert!(data.canonical_template_ids.is_empty());
        assert!(data.launched_projectiles.is_empty());
        assert!(data.script_states.is_empty());
    }

    #[test]
    fn current_patrol_link_round_trip_remaps_its_target() {
        let old_ai = EntityId::new_from_index_and_gen(7, 3);
        let old_target = EntityId::new_from_index_and_gen(8, 4);
        let mut data = EntitySaveData::empty();
        data.all_entities
            .extend([old_ai.inner(), old_target.inner()]);
        data.links.insert(
            old_ai.inner(),
            serde_json::to_value(Links {
                to_links: vec![ToLink {
                    to_template_id: 42,
                    to_entity_id: Some(WrappedEntityId(old_target)),
                    link: Link::AICurrentPatrol,
                }],
            })
            .unwrap(),
        );
        let mut world = World::new();

        let (_, entity_map) = data.instantiate(&mut world);

        let new_ai = entity_map[&old_ai];
        let new_target = entity_map[&old_target];
        let links = world.borrow::<View<Links>>().unwrap();
        let current = links
            .get(new_ai)
            .unwrap()
            .to_links
            .iter()
            .find(|link| link.link == Link::AICurrentPatrol)
            .unwrap();
        assert_eq!(current.to_entity_id.unwrap().0, new_target);
        assert_eq!(current.to_template_id, 42);
    }

    #[test]
    fn instantiate_restores_canonical_template_on_the_remapped_entity() {
        let old_entity = EntityId::new_from_index_and_gen(9, 2);
        let mut data = EntitySaveData::empty();
        data.all_entities.push(old_entity.inner());
        data.canonical_template_ids
            .insert(old_entity.inner(), -1358);
        let mut world = World::new();

        let (_, entity_map) = data.instantiate(&mut world);

        let new_entity = entity_map[&old_entity];
        let canonical = world
            .borrow::<View<RuntimePropCanonicalTemplateId>>()
            .unwrap();
        assert_eq!(canonical.get(new_entity).unwrap().0, -1358);
    }

    #[test]
    fn instantiate_restores_launched_projectile_marker_on_the_remapped_entity() {
        let old_entity = EntityId::new_from_index_and_gen(10, 2);
        let mut data = EntitySaveData::empty();
        data.all_entities.push(old_entity.inner());
        data.launched_projectiles.push(old_entity.inner());
        let mut world = World::new();

        let (_, entity_map) = data.instantiate(&mut world);

        let new_entity = entity_map[&old_entity];
        let launched = world
            .borrow::<View<RuntimePropLaunchedProjectile>>()
            .unwrap();
        assert!(launched.contains(new_entity));
        assert!(!launched.contains(old_entity));
    }

    #[test]
    fn script_state_envelope_round_trips_with_identity_and_version() {
        let old_entity = EntityId::new_from_index_and_gen(11, 4);
        let mut data = EntitySaveData::empty();
        data.script_states.push(SavedScriptState {
            entity_id: old_entity.inner(),
            identity: ScriptStateIdentity {
                script_key: "shock2vr.test".to_owned(),
                path: vec![2, 0, 1],
            },
            state: ScriptState {
                version: 3,
                payload: serde_json::json!({ "timer": 1.25 }),
            },
        });

        let decoded: EntitySaveData =
            serde_json::from_value(serde_json::to_value(&data).unwrap()).unwrap();

        assert_eq!(decoded.script_states, data.script_states);
    }
}
