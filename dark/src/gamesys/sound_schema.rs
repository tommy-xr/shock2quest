use std::{collections::HashMap, io};

use rand::{distributions::WeightedIndex, prelude::Distribution, thread_rng};
use shipyard::{Get, View, World};
use tracing::trace;

use crate::{
    properties::{PropSymName, PropTemplateId},
    ss2_chunk_file_reader::ChunkFileTableOfContents,
    ss2_common::{read_i32, read_string_with_size, read_u8, read_u32},
    ss2_entity_info::SystemShock2EntityInfo,
};

#[derive(Clone, Debug)]
pub struct SchemaSample {
    pub sample_name: String,
    pub frequency: u8,
}

#[derive(Clone, Debug)]
pub struct SoundSchema {
    name_to_samples: HashMap<String, Vec<SchemaSample>>,
    pub id_to_samples: HashMap<i32, Vec<SchemaSample>>,
}

impl SoundSchema {
    pub fn get_random_sample(&self, schema: &str) -> Option<String> {
        let maybe_samples = self.name_to_samples.get(&schema.to_ascii_lowercase());

        if let Some(samples) = maybe_samples {
            let mut rng = thread_rng();
            let weights = samples.iter().map(|s| s.frequency).collect::<Vec<u8>>();
            let weight_index = WeightedIndex::new(weights).unwrap();
            let idx = weight_index.sample(&mut rng);

            Some(samples[idx].sample_name.to_owned())
        } else {
            None
        }
    }

    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        reader: &mut T,
        gamesys_entity_info: &SystemShock2EntityInfo,
    ) -> SoundSchema {
        // Read SchSamp chunk
        let schema_chunk = table_of_contents.get_chunk("SchSamp".to_owned()).unwrap();
        let end = schema_chunk.offset + schema_chunk.length;
        let _ = reader.seek(io::SeekFrom::Start(schema_chunk.offset));

        trace!("starting at: {}", schema_chunk.offset);

        // 1) First, read the SchSamp chunk. This lets us build a map of entity id -> Vec<(filename, frequency)>
        let mut id_to_samples = HashMap::new();
        while reader.stream_position().unwrap() < end {
            let entity_id = read_i32(reader);
            let count = read_u32(reader);

            let mut samples = Vec::new();
            trace!("reading {} samples for: {}", count, entity_id);
            for _ in 0..count {
                let size = read_u32(reader);
                let sample_name = read_string_with_size(reader, size as usize);
                let frequency = read_u8(reader);

                trace!("-- {} | {}", &sample_name, frequency);
                samples.push(SchemaSample {
                    sample_name,
                    frequency,
                });
            }

            id_to_samples.insert(entity_id, samples);
        }

        // 2) Create database of entities - initializing the props, so we can read the sym name.
        // This will let us get the symname <-> EntityId relationship
        let mut world = World::new();
        let mut template_id_to_entity = HashMap::new();
        for (id, props) in &gamesys_entity_info.entity_to_properties {
            // Create the entity
            let entity = world.add_entity(());
            world.add_component(entity, PropTemplateId { template_id: *id });

            template_id_to_entity.insert(*id, entity);

            for prop in props {
                prop.initialize(&mut world, entity);
            }
        }

        // 3) Finally, we can use 1) and 2) above to create a map of string to schema samples
        let mut name_to_samples = HashMap::new();
        for (k, v) in &id_to_samples {
            let entity = template_id_to_entity.get(k).unwrap();

            let v_sym_name = world.borrow::<View<PropSymName>>().unwrap();
            let maybe_name = v_sym_name.get(*entity);

            if let Ok(name) = maybe_name {
                name_to_samples.insert(name.0.to_ascii_lowercase(), v.clone());
            }
        }

        trace!("{:?}", name_to_samples);
        SoundSchema {
            name_to_samples,
            id_to_samples,
        }
    }
}
