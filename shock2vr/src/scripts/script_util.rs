use crate::mission::entity_creator::initialize_entity_with_props;
use crate::{runtime_props::RuntimePropTransform, util::point3_to_vec3};
use cgmath::{Transform, point3};
use dark::{
    EnvSoundQuery,
    properties::{
        Link, Links, PropClassTag, PropSymName, PropTemplateId, PropTweqModelConfig, ToLink,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::audio::AudioHandle;
use shipyard::{Component, EntityId, Get, IntoIter, IntoWithId, View, World};
use std::collections::HashMap;

use super::{Effect, Message, MessagePayload};

pub fn is_message_turnon_or_turnoff(msg: &MessagePayload) -> bool {
    match msg {
        MessagePayload::TurnOn { from: _ } => true,
        MessagePayload::TurnOff { from: _ } => true,
        _ => false,
    }
}

pub fn get_all_links_with_template<TData>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Vec<(i32, TData)> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if let Some(data) = filter_fn(&link.link) {
                linked_entities.push((link.to_template_id, data))
            }
        }
    };

    linked_entities
}

pub fn get_first_link_with_template_and_data<TData: Clone>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Option<(i32, TData)> {
    let all_links = get_all_links_with_template(world, producing_entity_id, filter_fn);
    all_links
        .get(0)
        .map(|(template_id, data)| (*template_id, data.clone()))
}

pub fn for_each_link(
    world: &World,
    producing_entity_id: EntityId,
    foreach_fn: &mut dyn FnMut(&ToLink),
) {
    let links = world.borrow::<View<Links>>().unwrap();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            foreach_fn(link);
        }
    };
}

pub fn get_all_links_with_data<TData>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Vec<(EntityId, TData)> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if let (Some(to_entity_id), Some(data)) = (link.to_entity_id, filter_fn(&link.link)) {
                linked_entities.push((to_entity_id.0, data))
            }
        }
    };

    linked_entities
}

pub fn get_all_links_of_type(
    world: &World,
    producing_entity_id: EntityId,
    link_to_match: Link,
) -> Vec<EntityId> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            if link.link == link_to_match && link.to_entity_id.is_some() {
                linked_entities.push(link.to_entity_id.unwrap().0)
            }
        }
    };

    linked_entities
}

pub fn get_entities_by_name(world: &World, name: &str) -> Vec<EntityId> {
    let mut entities = Vec::new();
    world.run(|v_prop_symyname: View<PropSymName>| {
        for (id, symname) in v_prop_symyname.iter().with_id() {
            if name.eq_ignore_ascii_case(&symname.0) {
                entities.push(id);
            }
        }
    });

    entities
}

pub fn get_first_entity_by_name(world: &World, name: &str) -> Option<EntityId> {
    let entities = get_entities_by_name(world, name);
    entities.get(0).copied()
}

pub fn template_id_string(world: &World, entity_id: &EntityId) -> String {
    let v_template_id = world.borrow::<View<PropTemplateId>>().unwrap();
    let maybe_template = v_template_id.get(*entity_id);
    format!("{:?}", maybe_template)
}

pub fn get_first_link_with_data<TData: Copy>(
    world: &World,
    producing_entity_id: EntityId,
    filter_fn: fn(&Link) -> Option<TData>,
) -> Option<(EntityId, TData)> {
    let all_links = get_all_links_with_data(world, producing_entity_id, filter_fn);
    all_links.get(0).copied()
}

pub fn get_first_link_of_type(
    world: &World,
    producing_entity_id: EntityId,
    link_type: Link,
) -> Option<EntityId> {
    let all_links = get_all_links_of_type(world, producing_entity_id, link_type);
    all_links.get(0).copied()
}

pub fn get_all_switch_links(world: &World, producing_entity_id: EntityId) -> Vec<EntityId> {
    let links = world.borrow::<View<Links>>().unwrap();
    let mut linked_entities = Vec::new();
    if let Ok(switch_links) = links.get(producing_entity_id) {
        for link in &switch_links.to_links {
            match &link.link {
                dark::properties::Link::SwitchLink => {
                    if link.to_entity_id.is_some() {
                        linked_entities.push(link.to_entity_id.unwrap().0)
                    }
                }
                _ => (),
            }
        }
    };

    linked_entities
}
pub fn send_to_all_switch_links_and_self(
    world: &World,
    producing_entity_id: EntityId,
    message: MessagePayload,
) -> Effect {
    let send_to_switchlinks_eff =
        send_to_all_switch_links(world, producing_entity_id, message.clone());
    let send_to_self = Effect::Send {
        msg: Message {
            payload: message,
            to: producing_entity_id,
        },
    };
    Effect::Combined {
        effects: vec![send_to_switchlinks_eff, send_to_self],
    }
}

pub fn get_environmental_sound_query(
    world: &World,
    entity_id: EntityId,
    event_type: &str,
    additional_tags: Vec<(&str, &str)>,
) -> Option<EnvSoundQuery> {
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(entity_id)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);

    if !class_tags.is_empty() {
        let mut query = vec![("event", event_type)];
        query.append(&mut class_tags);
        query.append(&mut additional_tags.clone());
        Some(EnvSoundQuery::from_tag_values(query))
    } else {
        None
    }
}

pub fn play_environmental_sound(
    world: &World,
    entity_id: EntityId,
    event_type: &str,
    additional_tags: Vec<(&str, &str)>,
    audio_handle: AudioHandle,
) -> Effect {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let maybe_env_sound_query =
        get_environmental_sound_query(world, entity_id, event_type, additional_tags);

    if let Some(query) = maybe_env_sound_query {
        let position = v_transform
            .get(entity_id)
            .unwrap()
            .0
            .transform_point(point3(0.0, 0.0, 0.0));
        Effect::PlayEnvironmentalSound {
            audio_handle,
            query,
            position: point3_to_vec3(position),
        }
    } else {
        Effect::NoEffect
    }
}

pub fn send_to_all_switch_links(
    world: &World,
    producing_entity_id: EntityId,
    message: MessagePayload,
) -> Effect {
    let entities = get_all_switch_links(world, producing_entity_id);
    let effects = entities
        .iter()
        .map(|to| Effect::Send {
            msg: Message {
                to: *to,
                payload: message.clone(),
            },
        })
        .collect::<Vec<Effect>>();

    Effect::Combined { effects }
}

/// Hydrate a single template in a scratch world, grab a component, and drop it again.
/// Useful for debugging/inspecting template properties without spawning into the main world.
pub fn hydrate_template_component<T>(
    template_id: i32,
    entity_info: &SystemShock2EntityInfo,
) -> Option<T>
where
    T: Component + Clone + Send + Sync,
{
    let mut temp_world = World::new();
    let dummy_entity = temp_world.add_entity(());

    initialize_entity_with_props(
        template_id,
        entity_info,
        &mut temp_world,
        dummy_entity,
        &HashMap::new(), // empty obj_name_map for utility function
    );

    // Try to get the component from the hydrated entity
    let v_component = temp_world.borrow::<View<T>>();
    if let Ok(view) = v_component {
        if let Ok(component) = view.get(dummy_entity) {
            return Some(component.clone());
        }
    }

    None
}
pub fn invert(msg: MessagePayload) -> MessagePayload {
    match msg {
        MessagePayload::TurnOff { from } => MessagePayload::TurnOn { from },
        MessagePayload::TurnOn { from } => MessagePayload::TurnOff { from },
        m => m,
    }
}

pub fn change_to_last_model(world: &World, entity_id: EntityId) -> Effect {
    let v_prop_tweqmodelconfig = world.borrow::<View<PropTweqModelConfig>>().unwrap();

    if let Ok(model_config) = v_prop_tweqmodelconfig.get(entity_id) {
        let model_names = &model_config.model_names;
        if !model_names.is_empty() {
            let model_name = model_names.last().unwrap();
            Effect::ChangeModel {
                entity_id,
                model_name: model_name.to_owned(),
            }
        } else {
            Effect::NoEffect
        }
    } else {
        Effect::NoEffect
    }
}

pub fn change_to_first_model(world: &World, entity_id: EntityId) -> Effect {
    let v_prop_tweqmodelconfig = world.borrow::<View<PropTweqModelConfig>>().unwrap();

    if let Ok(model_config) = v_prop_tweqmodelconfig.get(entity_id) {
        let model_names = &model_config.model_names;
        if !model_names.is_empty() {
            let model_name = model_names.get(0).unwrap();
            Effect::ChangeModel {
                entity_id,
                model_name: model_name.to_owned(),
            }
        } else {
            Effect::NoEffect
        }
    } else {
        Effect::NoEffect
    }
}
