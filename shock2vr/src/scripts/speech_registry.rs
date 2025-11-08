use std::collections::HashMap;

use dark::properties::{PropSymName, PropVoiceIndex};
use shipyard::{Get, Unique, View, World, track::Untracked};

pub struct SpeechVoiceRegistry {
    label_to_index: HashMap<String, usize>,
}

impl SpeechVoiceRegistry {
    pub fn new() -> Self {
        Self {
            label_to_index: HashMap::new(),
        }
    }

    pub fn from_entity_info(entity_info: &dark::ss2_entity_info::SystemShock2EntityInfo) -> Self {
        let mut registry = SpeechVoiceRegistry::new();

        for props in entity_info.entity_to_properties.values() {
            let mut temp_world = World::new();
            let entity = temp_world.add_entity(());

            for prop in props {
                prop.initialize(&mut temp_world, entity);
            }

            let (label, index) = {
                let labels = temp_world.borrow::<View<PropSymName>>().ok();
                let indices = temp_world.borrow::<View<PropVoiceIndex>>().ok();

                if let (Some(labels), Some(indices)) = (labels, indices) {
                    let label = labels.get(entity).ok().map(|l| l.0.clone());
                    let index = indices.get(entity).ok().map(|i| i.0);
                    (label, index)
                } else {
                    (None, None)
                }
            };

            if let (Some(label), Some(index)) = (label, index) {
                if index >= 0 {
                    registry.insert(&label, index as usize);
                }
            }
        }

        registry
    }

    pub fn insert(&mut self, label: &str, index: usize) {
        self.label_to_index
            .insert(label.to_ascii_lowercase(), index);
    }

    pub fn lookup(&self, label: &str) -> Option<usize> {
        self.label_to_index
            .get(&label.to_ascii_lowercase())
            .copied()
    }
}

impl Unique for SpeechVoiceRegistry {
    type Tracking = Untracked;
}
