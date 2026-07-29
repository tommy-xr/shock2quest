mod entity_save_data;
mod held_item_save_data;
mod player_vitals;
mod save_data;

pub use entity_save_data::*;
pub use held_item_save_data::*;
pub use player_vitals::*;
pub use save_data::*;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
};

use dark::properties::{Link, Links};
use shipyard::{EntitiesView, EntityId, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    creature::RuntimePropHitBox,
    gui::GuiPropProxyEntity,
    mission::{GlobalTemplateIdMap, PlayerInfo},
    runtime_props::{
        RuntimePropCanonicalTemplateId, RuntimePropDeathPose, RuntimePropDoNotSerialize,
        RuntimePropEcologyState, RuntimePropSelectedAmmo,
    },
    scripts::{ScriptWorld, script_util},
    util::partition_map,
};

fn get_held_items(world: &World) -> HashSet<u64> {
    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let mut out = HashSet::new();

    if let Some(left_hand) = player.left_hand_entity_id {
        out.insert(left_hand.inner());
        add_contained_entities(&mut out, world, 2, left_hand);
    }

    if let Some(right_hand) = player.right_hand_entity_id {
        out.insert(right_hand.inner());
        add_contained_entities(&mut out, world, 2, right_hand);
    }

    out.insert(player.inventory_entity_id.inner());
    add_contained_entities(&mut out, world, 2, player.inventory_entity_id);

    out
}

fn add_contained_entities(
    set: &mut HashSet<u64>,
    world: &World,
    link_depth: u32,
    entity_id: EntityId,
) {
    if link_depth == 0 {
        return;
    }
    script_util::for_each_link(world, entity_id, &mut |link| {
        if matches!(link.link, Link::Contains(_)) {
            if let Some(to_ent_id) = link.to_entity_id {
                set.insert(to_ent_id.0.inner());
                add_contained_entities(set, world, link_depth - 1, to_ent_id.0);
            }
        }
    });
}

///
/// get_entities_to_filter_out
///
/// Returns a hashset of entities that should not be persisted (ie, proxy GUI entities),
/// because they are recreated by scripts
///
fn get_entities_to_filter_out(world: &World) -> HashSet<u64> {
    let _gui_proxy_entity = world.borrow::<View<GuiPropProxyEntity>>().unwrap();
    let _hitbox = world.borrow::<View<RuntimePropHitBox>>().unwrap();

    let do_not_serialize = world.borrow::<View<RuntimePropDoNotSerialize>>().unwrap();

    let mut out = HashSet::new();

    for (entity, _) in do_not_serialize.iter().with_id() {
        out.insert(entity.inner());
    }

    out
}

pub fn to_save_data(world: &World) -> (EntitySaveData, HeldItemSaveData) {
    to_save_data_with_scripts(world, None)
}

/// Serialize ECS-owned entity data plus opt-in private state from the mission's
/// script world. Non-mission scenes pass `None` and retain the legacy behavior.
pub fn to_save_data_with_scripts(
    world: &World,
    script_world: Option<&ScriptWorld>,
) -> (EntitySaveData, HeldItemSaveData) {
    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let template_id_to_entity_id = world.borrow::<UniqueView<GlobalTemplateIdMap>>().unwrap();

    let held_entities = get_held_items(world);

    let entities_to_filter = get_entities_to_filter_out(world);

    let script_states = script_world
        .map(ScriptWorld::save_states)
        .transpose()
        .unwrap_or_else(|error| panic!("unable to serialize script state: {error}"))
        .unwrap_or_default();
    let mut world_script_states = Vec::new();
    let mut held_script_states = Vec::new();
    for state in script_states {
        if entities_to_filter.contains(&state.entity_id) {
            continue;
        }
        if held_entities.contains(&state.entity_id) {
            held_script_states.push(state);
        } else {
            world_script_states.push(state);
        }
    }

    let v_links = world.borrow::<View<Links>>().unwrap();
    let v_selected_ammo = world.borrow::<View<RuntimePropSelectedAmmo>>().unwrap();
    let v_canonical_templates = world
        .borrow::<View<RuntimePropCanonicalTemplateId>>()
        .unwrap();
    let v_ecology_states = world.borrow::<View<RuntimePropEcologyState>>().unwrap();
    let v_entities = world.borrow::<EntitiesView>().unwrap();

    let (all_properties, _, _) = dark::properties::get::<File>();

    let mut all_world_entities = Vec::new();
    let mut all_held_entities: Vec<u64> = Vec::new();

    for entity in v_entities.iter() {
        if entities_to_filter.contains(&entity.inner()) {
            continue;
        }

        if held_entities.contains(&entity.inner()) {
            all_held_entities.push(entity.inner());
        } else {
            all_world_entities.push(entity.inner());
        }
    }

    let mut world_serialized_properties = HashMap::new();
    let mut held_serialized_properties = HashMap::new();
    for prop in all_properties {
        let raw_serialized = prop.serialize(world);

        // Filter out entities we don't care about, first...
        let (_ignored, serialized) =
            partition_map(raw_serialized, |ent| entities_to_filter.contains(ent));

        let (held_serialized, world_serialized) =
            partition_map(serialized, |ent| held_entities.contains(ent));

        world_serialized_properties.insert(prop.name(), world_serialized);
        held_serialized_properties.insert(prop.name(), held_serialized);
    }

    let mut world_serialized_links = HashMap::new();
    let mut held_serialized_links = HashMap::new();
    for (entity_id, links) in v_links.iter().with_id() {
        let serialized = serde_json::to_value(links).unwrap();

        if held_entities.contains(&entity_id.inner()) {
            held_serialized_links.insert(entity_id.inner(), serialized);
        } else {
            world_serialized_links.insert(entity_id.inner(), serialized);
        }
    }

    let v_death_poses = world.borrow::<View<RuntimePropDeathPose>>().unwrap();
    let mut world_death_poses = HashMap::new();
    let mut held_death_poses = HashMap::new();
    for (entity_id, death_pose) in v_death_poses.iter().with_id() {
        if entities_to_filter.contains(&entity_id.inner()) {
            continue;
        }
        if held_entities.contains(&entity_id.inner()) {
            held_death_poses.insert(entity_id.inner(), death_pose.clone());
        } else {
            world_death_poses.insert(entity_id.inner(), death_pose.clone());
        }
    }

    let raw_selected_ammo: HashMap<u64, usize> = v_selected_ammo
        .iter()
        .with_id()
        .filter(|(entity_id, _)| !entities_to_filter.contains(&entity_id.inner()))
        .map(|(entity_id, selected)| (entity_id.inner(), selected.0))
        .collect();
    let (held_selected_ammo, world_selected_ammo) = partition_map(raw_selected_ammo, |entity_id| {
        held_entities.contains(entity_id)
    });
    let raw_canonical_templates: HashMap<u64, i32> = v_canonical_templates
        .iter()
        .with_id()
        .filter(|(entity_id, _)| !entities_to_filter.contains(&entity_id.inner()))
        .map(|(entity_id, canonical)| (entity_id.inner(), canonical.0))
        .collect();
    let (held_canonical_templates, world_canonical_templates) =
        partition_map(raw_canonical_templates, |entity_id| {
            held_entities.contains(entity_id)
        });
    let raw_ecology_states: HashMap<u64, RuntimePropEcologyState> = v_ecology_states
        .iter()
        .with_id()
        .filter(|(entity_id, _)| !entities_to_filter.contains(&entity_id.inner()))
        .map(|(entity_id, state)| (entity_id.inner(), *state))
        .collect();
    let (held_ecology_states, world_ecology_states) =
        partition_map(raw_ecology_states, |entity_id| {
            held_entities.contains(entity_id)
        });

    let world_entity_data = EntitySaveData {
        properties: world_serialized_properties,
        template_id_to_entity_id: template_id_to_entity_id.0.clone(),
        links: world_serialized_links,
        all_entities: all_world_entities,
        death_poses: world_death_poses,
        selected_ammo: world_selected_ammo,
        canonical_template_ids: world_canonical_templates,
        script_states: world_script_states,
        ecology_states: world_ecology_states,
    };

    let held_entity_data = EntitySaveData {
        all_entities: all_held_entities,
        template_id_to_entity_id: HashMap::new(),
        links: held_serialized_links,
        properties: held_serialized_properties,
        death_poses: held_death_poses,
        selected_ammo: held_selected_ammo,
        canonical_template_ids: held_canonical_templates,
        script_states: held_script_states,
        ecology_states: held_ecology_states,
    };

    let held_metadata = HeldItemSaveData {
        entity_in_left_hand: player.left_hand_entity_id.map(|ent| ent.inner()),
        entity_in_right_hand: player.right_hand_entity_id.map(|ent| ent.inner()),
        held_entities: held_entity_data,
        inventory_entity: Some(player.inventory_entity_id.inner()),
    };
    (world_entity_data, held_metadata)
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{PropSpawn, SpawnFlags, ToLink, WrappedEntityId};
    use shipyard::{Get, View};

    use super::*;

    #[test]
    fn ecology_save_round_trip_preserves_supply_child_ownership_and_timers() {
        let mut world = World::new();
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        let child = world.add_entity(());
        let marker = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: -196,
                to_entity_id: Some(WrappedEntityId(child)),
                link: Link::Spawned,
            }],
        });
        let generator = world.add_entity((
            PropSpawn {
                object_names: [
                    "OG-Pipe".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                odds: [100, 0, 0, 0],
                flags: SpawnFlags::POP_LIMIT,
                supply: 3,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 147,
                    to_entity_id: Some(WrappedEntityId(marker)),
                    link: Link::SpawnPoint,
                }],
            },
        ));
        let ecology = world.add_entity(RuntimePropEcologyState {
            seconds_until_poll: 4.5,
            recovery_seconds_remaining: Some(88.0),
        });
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        world.add_unique(GlobalTemplateIdMap(HashMap::from([
            (71, WrappedEntityId(ecology)),
            (73, WrappedEntityId(generator)),
            (147, WrappedEntityId(marker)),
        ])));

        let (save, _) = to_save_data(&world);
        let mut loaded = World::new();
        let (_, remapped) = save.instantiate(&mut loaded);

        let loaded_generator = remapped[&generator];
        let loaded_marker = remapped[&marker];
        let loaded_child = remapped[&child];
        let loaded_ecology = remapped[&ecology];
        let spawns = loaded.borrow::<View<PropSpawn>>().unwrap();
        assert_eq!(spawns.get(loaded_generator).unwrap().supply, 3);
        drop(spawns);
        let links = loaded.borrow::<View<Links>>().unwrap();
        assert!(
            links
                .get(loaded_marker)
                .unwrap()
                .to_links
                .iter()
                .any(|link| {
                    link.link == Link::Spawned
                        && link
                            .to_entity_id
                            .is_some_and(|target| target.0 == loaded_child)
                })
        );
        drop(links);
        let runtime_states = loaded.borrow::<View<RuntimePropEcologyState>>().unwrap();
        assert_eq!(
            *runtime_states.get(loaded_ecology).unwrap(),
            RuntimePropEcologyState {
                seconds_until_poll: 4.5,
                recovery_seconds_remaining: Some(88.0),
            }
        );
    }
}
