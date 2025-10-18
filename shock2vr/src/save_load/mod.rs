mod entity_save_data;
mod held_item_save_data;
mod save_data;

pub use entity_save_data::*;
pub use held_item_save_data::*;
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
    runtime_props::RuntimePropDoNotSerialize,
    scripts::script_util,
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
    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let template_id_to_entity_id = world.borrow::<UniqueView<GlobalTemplateIdMap>>().unwrap();

    let held_entities = get_held_items(world);

    let entities_to_filter = get_entities_to_filter_out(world);

    let v_links = world.borrow::<View<Links>>().unwrap();
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

    let world_entity_data = EntitySaveData {
        properties: world_serialized_properties,
        template_id_to_entity_id: template_id_to_entity_id.0.clone(),
        links: world_serialized_links,
        all_entities: all_world_entities,
    };

    let held_entity_data = EntitySaveData {
        all_entities: all_held_entities,
        template_id_to_entity_id: HashMap::new(),
        links: held_serialized_links,
        properties: held_serialized_properties,
    };

    let held_metadata = HeldItemSaveData {
        entity_in_left_hand: player.left_hand_entity_id.map(|ent| ent.inner()),
        entity_in_right_hand: player.right_hand_entity_id.map(|ent| ent.inner()),
        held_entities: held_entity_data,
        inventory_entity: Some(player.inventory_entity_id.inner()),
    };
    (world_entity_data, held_metadata)
}
