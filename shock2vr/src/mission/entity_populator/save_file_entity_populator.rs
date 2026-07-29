/**
 * save_file_entity_populator.rs
 *
 * An implementation of EntityPopulator that creates entities based on the entity data in save file.
 *
 */
use dark::properties::{
    Link, Links, PropEcoState, PropEcoType, PropEcology, PropSpawn, WrappedEntityId,
};
use shipyard::{Get, View, ViewMut, World};
use std::collections::HashMap;

use dark::ss2_entity_info::SystemShock2EntityInfo;

use crate::{mission::entity_creator, save_load::EntitySaveData};

use super::EntityPopulator;

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
    ) -> HashMap<i32, WrappedEntityId> {
        let world_entity_data = &self.save_data;
        let (template_to_entity, _) = world_entity_data.instantiate(world);
        restore_newly_parsed_authored_ecology(
            gamesys_entity_info,
            level_entity_info,
            obj_name_map,
            &template_to_entity,
            world,
        );
        template_to_entity
    }
}

/// Backfill retail ecology data omitted by saves written before these chunks
/// were understood.
///
/// Saves deliberately own mutable runtime state, so loading cannot generally
/// reapply the mission file over a restored entity. These four properties and
/// `SpawnPoint` links are different: old builds could neither deserialize nor
/// mutate them, and therefore could not have serialized them. Restore only a
/// missing component/link, only for a concrete mission object which still
/// exists in the save. Deleted generators/ecologies stay deleted, while newer
/// saves retain their live supply, state, and spawned-child graph.
fn restore_newly_parsed_authored_ecology(
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
