/**
 * save_file_entity_populator.rs
 *
 * An implementation of EntityPopulator that creates entities based on the entity data in save file.
 *
 */
use dark::properties::{
    Link, Links, PropBaseTechDesc, PropChemicalNeeded, PropDoorTimer, PropEcoState, PropEcoType,
    PropEcology, PropObjLookString, PropRequiredTechDesc, PropResearchReport, PropResearchText,
    PropResearchTime, PropRotatingDoor, PropSpawn, WrappedEntityId,
};
use shipyard::{Get, View, ViewMut, World};
use std::collections::HashMap;

use dark::ss2_entity_info::SystemShock2EntityInfo;

use crate::{mission::entity_creator, save_load::EntitySaveData};

use super::{EntityPopulation, EntityPopulator};

pub struct SaveFileEntityPopulator {
    pub save_data: EntitySaveData,
}

impl<'a> SaveFileEntityPopulator {
    pub fn create(save_data: EntitySaveData) -> SaveFileEntityPopulator {
        SaveFileEntityPopulator { save_data }
    }
}

impl EntityPopulator for SaveFileEntityPopulator {
    fn populate(
        &self,
        gamesys_entity_info: &SystemShock2EntityInfo,
        level_entity_info: &SystemShock2EntityInfo,
        obj_name_map: &HashMap<i32, String>, // name override map
        world: &mut World,
    ) -> EntityPopulation {
        let world_entity_data = &self.save_data;
        let (template_to_entity, entity_id_map) = world_entity_data.instantiate(world);
        restore_newly_parsed_authored_data(
            gamesys_entity_info,
            level_entity_info,
            obj_name_map,
            &template_to_entity,
            world,
        );
        EntityPopulation {
            template_to_entity_id: template_to_entity,
            entity_id_map,
            script_states: world_entity_data.script_states.clone(),
        }
    }
}

/// Backfill retail data omitted by saves written before these chunks were
/// understood.
///
/// Saves deliberately own mutable runtime state, so loading cannot generally
/// reapply the mission file over a restored entity. The ecology and research
/// properties below are different: old builds could neither deserialize nor
/// mutate them, and therefore could not have serialized them. This includes
/// RotDoor/DoorTimer data needed to repair transforms already serialized by an
/// old runtime. Restore only a missing component/link, only for a concrete
/// mission object which still exists in the save. Deleted generators/ecologies
/// stay deleted, while newer saves retain their live values and spawned-child
/// graph.
fn restore_newly_parsed_authored_data(
    gamesys_entity_info: &SystemShock2EntityInfo,
    level_entity_info: &SystemShock2EntityInfo,
    obj_name_map: &HashMap<i32, String>,
    template_to_entity: &HashMap<i32, WrappedEntityId>,
    world: &mut World,
) {
    let concrete_entities: Vec<_> = template_to_entity
        .iter()
        .filter(|(template_id, _)| {
            **template_id > 0
                && level_entity_info
                    .entity_to_properties
                    .contains_key(template_id)
        })
        .map(|(template_id, entity_id)| (*template_id, *entity_id))
        .collect();
    if concrete_entities.is_empty() {
        return;
    }

    // Hydrate the authored definitions in a scratch ECS. This lets the normal
    // property inheritance/link merging code decide the effective value while
    // keeping every unrelated saved component untouched.
    let mut authored_world = World::new();
    let authored_entities: HashMap<_, _> = concrete_entities
        .iter()
        .map(|(template_id, _)| (*template_id, authored_world.add_entity(())))
        .collect();
    for (template_id, authored_entity) in &authored_entities {
        entity_creator::initialize_entity_with_props(
            *template_id,
            gamesys_entity_info,
            &mut authored_world,
            *authored_entity,
            obj_name_map,
        );
        entity_creator::initialize_links_for_entity(
            *template_id,
            *authored_entity,
            gamesys_entity_info,
            template_to_entity,
            &mut authored_world,
        );
    }

    macro_rules! restore_missing_component {
        ($component:ty) => {{
            let missing = if let Ok(authored) = authored_world.borrow::<View<$component>>() {
                let restored = world.borrow::<View<$component>>().ok();
                concrete_entities
                    .iter()
                    .filter_map(|(template_id, restored_entity)| {
                        let authored_entity = authored_entities[template_id];
                        if restored
                            .as_ref()
                            .is_some_and(|restored| restored.get(restored_entity.0).is_ok())
                        {
                            None
                        } else {
                            authored
                                .get(authored_entity)
                                .ok()
                                .cloned()
                                .map(|value| (restored_entity.0, value))
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            for (entity, value) in missing {
                world.add_component(entity, value);
            }
        }};
    }

    restore_missing_component!(PropEcology);
    restore_missing_component!(PropEcoType);
    restore_missing_component!(PropEcoState);
    restore_missing_component!(PropSpawn);
    restore_missing_component!(PropBaseTechDesc);
    restore_missing_component!(PropChemicalNeeded);
    restore_missing_component!(PropObjLookString);
    restore_missing_component!(PropRequiredTechDesc);
    restore_missing_component!(PropResearchReport);
    restore_missing_component!(PropResearchText);
    restore_missing_component!(PropResearchTime);
    restore_missing_component!(PropRotatingDoor);
    restore_missing_component!(PropDoorTimer);

    let authored_spawn_points = {
        let authored_links = authored_world.borrow::<View<Links>>().unwrap();
        concrete_entities
            .iter()
            .filter_map(|(template_id, restored_entity)| {
                let authored_entity = authored_entities[template_id];
                let spawn_points = authored_links
                    .get(authored_entity)
                    .ok()?
                    .to_links
                    .iter()
                    .filter(|link| link.link == Link::SpawnPoint)
                    .cloned()
                    .collect::<Vec<_>>();
                (!spawn_points.is_empty()).then_some((restored_entity.0, spawn_points))
            })
            .collect::<Vec<_>>()
    };
    let mut missing_link_components = Vec::new();
    if let Ok(mut restored_links) = world.borrow::<ViewMut<Links>>() {
        for (entity, spawn_points) in authored_spawn_points {
            if let Ok(links) = (&mut restored_links).get(entity) {
                for spawn_point in spawn_points {
                    let already_present = links.to_links.iter().any(|link| {
                        link.link == Link::SpawnPoint
                            && link.to_template_id == spawn_point.to_template_id
                    });
                    if !already_present {
                        links.to_links.push(spawn_point);
                    }
                }
            } else {
                missing_link_components.push((
                    entity,
                    Links {
                        to_links: spawn_points,
                    },
                ));
            }
        }
    } else {
        missing_link_components.extend(
            authored_spawn_points
                .into_iter()
                .map(|(entity, to_links)| (entity, Links { to_links })),
        );
    }
    for (entity, links) in missing_link_components {
        world.add_component(entity, links);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        PropResearchReport, PropResearchText, PropResearchTime, PropRotatingDoor,
    };

    use super::*;

    #[test]
    fn legacy_unlooted_researchable_backfills_newly_parsed_metadata() {
        let mut merged_entity_info = SystemShock2EntityInfo::empty();
        merged_entity_info.entity_to_properties.insert(
            675,
            vec![
                Arc::new(Box::new(PropResearchTime(600))),
                Arc::new(Box::new(PropResearchReport(0x10))),
                Arc::new(Box::new(PropResearchText("AATText".to_owned()))),
            ],
        );
        let mut level_entity_info = SystemShock2EntityInfo::empty();
        level_entity_info.entity_to_properties.insert(675, vec![]);

        let mut world = World::new();
        let toxin = world.add_entity(PropResearchReport(0x20));
        let template_to_entity = HashMap::from([(675, WrappedEntityId(toxin))]);

        restore_newly_parsed_authored_data(
            &merged_entity_info,
            &level_entity_info,
            &HashMap::new(),
            &template_to_entity,
            &mut world,
        );

        assert_eq!(
            world
                .borrow::<View<PropResearchTime>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            600
        );
        assert_eq!(
            world
                .borrow::<View<PropResearchText>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            "AATText"
        );
        assert_eq!(
            world
                .borrow::<View<PropResearchReport>>()
                .unwrap()
                .get(toxin)
                .unwrap()
                .0,
            0x20,
            "serialized live values must win over authored defaults"
        );
    }

    #[test]
    fn legacy_rotating_door_backfills_newly_parsed_authored_motion() {
        let mut merged_entity_info = SystemShock2EntityInfo::empty();
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        merged_entity_info.entity_to_properties.insert(
            85,
            vec![Arc::new(Box::new(PropRotatingDoor {
                door_type: 0,
                closed: 0.0,
                open: 95.0,
                speed: 1.0,
                axis: 0,
                state: 0,
                clockwise: false,
                base_closed_location: vec3(0.1, -12.75, -23.7),
                base_open_location: vec3(0.1, -11.9, -24.7),
                base_location: vec3(0.1, -12.75, -23.7),
                base_rotation: identity,
                base_closed_rotation: identity,
                base_open_rotation: identity,
                progress: 0.0,
            }))],
        );
        let mut level_entity_info = SystemShock2EntityInfo::empty();
        level_entity_info.entity_to_properties.insert(85, vec![]);

        let mut world = World::new();
        let hatch = world.add_entity(());
        let template_to_entity = HashMap::from([(85, WrappedEntityId(hatch))]);

        restore_newly_parsed_authored_data(
            &merged_entity_info,
            &level_entity_info,
            &HashMap::new(),
            &template_to_entity,
            &mut world,
        );

        let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
        let restored = doors.get(hatch).unwrap();
        assert_eq!(restored.base_closed_location, vec3(0.1, -12.75, -23.7));
        assert_eq!(restored.open, 95.0);
    }
}
