use dark::properties::{PropClassTag, PropSpeechVoice, PropVoiceIndex};
use shipyard::{EntityId, Get, UniqueView, View, World};
use tracing::trace;

use crate::scripts::speech_registry::SpeechVoiceRegistry;

/// Resolves the voice index for an entity, checking in order:
/// 1. Direct PropVoiceIndex property
/// 2. PropSpeechVoice label resolved through the registry
/// 3. Inferred from creature type tag
pub fn resolve_entity_voice_index(world: &World, entity_id: EntityId) -> Option<usize> {
    // First check for direct voice index
    if let Ok(v_voice_index) = world.borrow::<View<PropVoiceIndex>>() {
        if let Ok(voice_prop) = v_voice_index.get(entity_id) {
            // Only use the index if it's non-negative (-1 is a sentinel for "unset")
            if voice_prop.0 >= 0 {
                let index = voice_prop.0 as usize;
                drop(v_voice_index);
                trace!("Entity {:?} has direct voice index: {}", entity_id, index);
                return Some(index);
            }
        }
        drop(v_voice_index);
    }

    // Then check for speech voice label
    if let Ok(v_speech_voice) = world.borrow::<View<PropSpeechVoice>>() {
        if let Ok(voice_label) = v_speech_voice.get(entity_id) {
            let label = voice_label.0.clone();
            drop(v_speech_voice);

            if let Some(index) = lookup_voice_index_by_label(world, &label) {
                trace!(
                    "Entity {:?} resolved voice '{}' to index {}",
                    entity_id, label, index
                );
                return Some(index);
            } else {
                trace!(
                    "Entity {:?} has voice label '{}' but couldn't resolve it",
                    entity_id, label
                );
            }
        } else {
            drop(v_speech_voice);
        }
    }

    // Finally try to infer from creature type
    if let Some(index) = infer_voice_index_from_creature_type(world, entity_id) {
        trace!(
            "Entity {:?} inferred voice index {} from creature type",
            entity_id, index
        );
        return Some(index);
    }

    trace!("Entity {:?} has no resolvable voice index", entity_id);
    None
}

/// Looks up a voice index by its label string (e.g., "voncegrunt" -> 2)
pub fn lookup_voice_index_by_label(world: &World, label: &str) -> Option<usize> {
    world
        .borrow::<UniqueView<SpeechVoiceRegistry>>()
        .ok()
        .and_then(|registry| registry.lookup(label))
}

/// Attempts to infer voice index from creature type class tag
pub fn infer_voice_index_from_creature_type(world: &World, entity_id: EntityId) -> Option<usize> {
    if let Ok(class_tags) = world.borrow::<View<PropClassTag>>() {
        if let Ok(tags) = class_tags.get(entity_id) {
            for (tag, value) in tags.class_tags() {
                if tag.eq_ignore_ascii_case("creaturetype") {
                    // Try looking up "v" + creature type (e.g., "vgrunt")
                    let label = format!("v{}", value);
                    drop(class_tags);
                    return lookup_voice_index_by_label(world, &label);
                }
            }
        }
        drop(class_tags);
    }
    None
}
