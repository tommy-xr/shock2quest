use std::{collections::HashMap, fs::File};

use dark::properties::{Links, WrappedEntityId};
use engine::game_log;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, World};

use crate::runtime_props::{
    RuntimePropCanonicalTemplateId, RuntimePropDeathPose, RuntimePropLaunchedProjectile,
    RuntimePropPlayerFiredProjectile, RuntimePropSelectedAmmo,
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
    /// Exact weapon membership in right/left thigh slots.
    #[serde(default)]
    pub holstered: HashMap<u64, crate::runtime_props::RuntimePropHolstered>,
    #[serde(default)]
    pub shoulder_weapons: HashMap<u64, crate::runtime_props::RuntimePropShoulderWeapon>,
    /// Stable gamesys archetype for entities whose `PropTemplateId` is a
    /// positive, mission-local object ID.
    #[serde(default)]
    pub canonical_template_ids: HashMap<u64 /* entity id */, i32>,
    /// Entities created through Dark's `launchProjectile` path. The marker
    /// restores dynamic authored physics and keeps empty animation players
    /// from taking velocity ownership after load.
    #[serde(default)]
    pub launched_projectiles: Vec<u64 /* entity id */>,
    /// Player-owned shots retain their shooter collision filter after load.
    /// This is separate from launch provenance: enemy shots are launched too.
    #[serde(default)]
    pub player_fired_projectiles: Vec<u64 /* entity id */>,
    #[serde(default)]
    pub projectile_velocities: HashMap<u64, cgmath::Vector3<f32>>,
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
            holstered: HashMap::new(),
            shoulder_weapons: HashMap::new(),
            canonical_template_ids: HashMap::new(),
            launched_projectiles: Vec::new(),
            player_fired_projectiles: Vec::new(),
            projectile_velocities: HashMap::new(),
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

        for (old, velocity) in &self.projectile_velocities {
            if let Some(new) =
                EntityId::from_inner(*old).and_then(|id| old_entity_id_to_new_entity_id.get(&id))
            {
                world.add_component(
                    *new,
                    crate::runtime_props::RuntimePropProjectileVelocity(*velocity),
                );
            }
        }

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
        for (old, slot) in &self.shoulder_weapons {
            if let Some(new) =
                EntityId::from_inner(*old).and_then(|id| old_entity_id_to_new_entity_id.get(&id))
            {
                world.add_component(*new, *slot);
            }
        }
        for (old, slot) in &self.holstered {
            if let Some(new) =
                EntityId::from_inner(*old).and_then(|id| old_entity_id_to_new_entity_id.get(&id))
            {
                world.add_component(*new, *slot);
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
        for old_entity_id in &self.player_fired_projectiles {
            let old_entity_id = EntityId::from_inner(*old_entity_id).unwrap();
            if let Some(new_entity_id) = old_entity_id_to_new_entity_id.get(&old_entity_id) {
                world.add_component(*new_entity_id, RuntimePropPlayerFiredProjectile);
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
    fn player_projectile_ownership_round_trips_without_marking_enemy_shots() {
        use crate::mission::{GlobalTemplateIdMap, PlayerInfo};
        use crate::runtime_props::{RuntimePropDoNotSerialize, RuntimePropPlayerFiredProjectile};
        let mut world = World::new();
        let player = world.add_entity(RuntimePropDoNotSerialize);
        let inventory = world.add_entity(());
        let shot = world.add_entity((
            RuntimePropPlayerFiredProjectile,
            RuntimePropLaunchedProjectile,
        ));
        let held_shot = world.add_entity((
            RuntimePropPlayerFiredProjectile,
            RuntimePropLaunchedProjectile,
        ));
        let enemy_shot = world.add_entity(RuntimePropLaunchedProjectile);
        let excluded =
            world.add_entity((RuntimePropPlayerFiredProjectile, RuntimePropDoNotSerialize));
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(PlayerInfo {
            pos: cgmath::vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            inventory_entity_id: inventory,
            left_hand_entity_id: Some(held_shot),
            right_hand_entity_id: None,
        });
        let (saved, held) = crate::save_load::to_save_data(&world);
        assert!(!saved.all_entities.contains(&excluded.inner()));
        assert!(!saved.all_entities.contains(&held_shot.inner()));
        for (data, expected) in [(saved, shot), (held.held_entities, held_shot)] {
            let encoded = serde_json::to_string(&data).unwrap();
            let decoded: EntitySaveData = serde_json::from_str(&encoded).unwrap();
            let mut restored = World::new();
            for _ in 0..20 {
                restored.add_entity(());
            }
            let (_, remapped) = decoded.instantiate(&mut restored);
            let markers = restored
                .borrow::<View<RuntimePropPlayerFiredProjectile>>()
                .unwrap();
            assert_ne!(remapped[&expected], expected);
            assert!(
                markers.contains(remapped[&expected]),
                "saved ownership must reach the remapped shot"
            );
            if let Some(enemy) = remapped.get(&enemy_shot) {
                assert!(
                    !markers.contains(*enemy),
                    "enemy projectiles must still hit the player"
                );
                assert!(
                    restored
                        .borrow::<View<RuntimePropLaunchedProjectile>>()
                        .unwrap()
                        .contains(*enemy),
                    "launch provenance must survive without granting player ownership"
                );
            }
            assert!(!remapped.contains_key(&excluded));
        }
    }

    #[test]
    fn holster_membership_and_ammo_remap_to_the_same_saved_weapon() {
        let mut source = World::new();
        let weapon = source.add_entity(());
        let mut data = EntitySaveData::empty();
        data.all_entities.push(weapon.inner());
        data.holstered.insert(
            weapon.inner(),
            crate::runtime_props::RuntimePropHolstered {
                slot: 1,
                held_extent: Some(0.3),
            },
        );
        data.selected_ammo.insert(weapon.inner(), 2);
        data.shoulder_weapons.insert(
            weapon.inner(),
            crate::runtime_props::RuntimePropShoulderWeapon(0),
        );
        let data: EntitySaveData =
            serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        let mut restored = World::new();
        restored.add_entity(());
        let (_, map) = data.instantiate(&mut restored);
        let id = map[&weapon];
        assert_eq!(
            restored
                .borrow::<View<crate::runtime_props::RuntimePropShoulderWeapon>>()
                .unwrap()
                .get(id)
                .unwrap()
                .0,
            0
        );
        assert_eq!(
            restored
                .borrow::<View<crate::runtime_props::RuntimePropHolstered>>()
                .unwrap()
                .get(id)
                .unwrap()
                .slot,
            1
        );
        assert_eq!(
            restored
                .borrow::<View<RuntimePropSelectedAmmo>>()
                .unwrap()
                .get(id)
                .unwrap()
                .0,
            2
        );
    }

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

    /// A gun worn down by firing must load back worn: condition rides the
    /// generic property save, so this covers the whole `P$GunState` chunk.
    #[test]
    fn a_degraded_weapon_condition_survives_a_save_and_load() {
        let mut source_world = World::new();
        let gun = source_world.add_entity((dark::properties::PropGunState {
            ammo: 7,
            condition: 93.0,
            setting: 1,
            modification: 0,
            silence_value: 0.0,
        },));

        // The same generic property serialization `to_save_data` performs.
        let (all_properties, _, _) = dark::properties::get::<File>();
        let mut save = EntitySaveData::empty();
        save.all_entities.push(gun.inner());
        for prop in all_properties {
            save.properties
                .insert(prop.name(), prop.serialize(&source_world));
        }

        let mut loaded_world = World::new();
        let (_, old_to_new) = save.instantiate(&mut loaded_world);

        let states = loaded_world
            .borrow::<View<dark::properties::PropGunState>>()
            .unwrap();
        let loaded = states.get(old_to_new[&gun]).unwrap();
        assert_eq!(loaded.condition, 93.0);
        assert_eq!(loaded.ammo, 7);
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
