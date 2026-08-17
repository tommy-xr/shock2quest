use std::{collections::HashMap, io};

use rand::{Rng, distributions::WeightedIndex, prelude::Distribution, thread_rng};
use shipyard::{Get, View, World};
use tracing::trace;

use crate::{
    properties::{PropSchemaPlayParams, PropSymName},
    ss2_chunk_file_reader::ChunkFileTableOfContents,
    ss2_common::{read_i32, read_string_with_size, read_u8, read_u32},
    ss2_entity_info::SystemShock2EntityInfo,
};

#[derive(Clone, Debug)]
pub struct SchemaSample {
    pub sample_name: String,
    pub frequency: u8,
}

/// A concrete sample plus the authored playback values resolved from its
/// schema inheritance chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSoundSchema {
    pub sample_name: String,
    pub volume_millibels: i32,
    pub pan_millibels: i32,
}

impl ResolvedSoundSchema {
    /// Convert Dark's millibel gain to the linear amplitude used by rodio.
    pub fn linear_gain(&self) -> f32 {
        10.0_f32.powf(self.volume_millibels.clamp(-10_000, 0) as f32 / 2000.0)
    }

    /// Convert Dark's fixed pan into per-channel amplitude multipliers.
    pub fn channel_gains(&self) -> [f32; 2] {
        let pan = self.pan_millibels.clamp(-10_000, 10_000);
        if pan < 0 {
            [1.0, 10.0_f32.powf(pan as f32 / 2000.0)]
        } else if pan > 0 {
            [10.0_f32.powf(-(pan as f32) / 2000.0), 1.0]
        } else {
            [1.0, 1.0]
        }
    }
}

#[derive(Clone, Debug)]
pub struct SoundSchema {
    name_to_schema_id: HashMap<String, i32>,
    pub id_to_samples: HashMap<i32, Vec<SchemaSample>>,
    id_to_play_params: HashMap<i32, PropSchemaPlayParams>,
}

impl SoundSchema {
    pub fn get_random_sample(&self, schema: &str) -> Option<String> {
        let id = *self.name_to_schema_id.get(&schema.to_ascii_lowercase())?;
        let samples = self.id_to_samples.get(&id)?;
        let mut rng = thread_rng();
        let weights = samples.iter().map(|s| s.frequency).collect::<Vec<u8>>();
        let weight_index = WeightedIndex::new(weights).unwrap();
        Some(samples[weight_index.sample(&mut rng)].sample_name.clone())
    }

    pub fn resolve(&self, schema: &str) -> Option<ResolvedSoundSchema> {
        let id = *self.name_to_schema_id.get(&schema.to_ascii_lowercase())?;
        self.resolve_id(id)
    }

    pub fn resolve_id(&self, schema_id: i32) -> Option<ResolvedSoundSchema> {
        let samples = self.id_to_samples.get(&schema_id)?;
        let mut rng = thread_rng();
        let weights = samples.iter().map(|s| s.frequency).collect::<Vec<u8>>();
        let weight_index = WeightedIndex::new(weights).unwrap();
        let sample = &samples[weight_index.sample(&mut rng)];

        self.resolve_sample(schema_id, sample.sample_name.clone())
    }

    /// Attach a schema's play parameters to a sample already selected by a
    /// caller. Speech uses this to retain its established candidate/sample
    /// selection rules without losing volume and pan.
    pub fn resolve_sample(
        &self,
        schema_id: i32,
        sample_name: String,
    ) -> Option<ResolvedSoundSchema> {
        self.id_to_samples.get(&schema_id)?;
        let params = self
            .id_to_play_params
            .get(&schema_id)
            .cloned()
            .unwrap_or_default();
        let mut rng = thread_rng();

        let pan_millibels = if params.flags & PropSchemaPlayParams::PAN_POSITION != 0 {
            params.pan
        } else if params.flags & PropSchemaPlayParams::PAN_RANGE != 0 {
            let range = params.pan.clamp(-10_000, 10_000).abs();
            rng.gen_range(-range..=range)
        } else {
            0
        };

        Some(ResolvedSoundSchema {
            sample_name,
            volume_millibels: params.volume,
            pan_millibels,
        })
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
        let template_id_to_entity =
            gamesys_entity_info
                .initialize_world_with_entities(&mut world, HashMap::new(), |_| true);

        // 3) Finally, use the hydrated world to associate schema ids with names
        // and inherited playback values.
        let mut name_to_schema_id = HashMap::new();
        let mut id_to_play_params = HashMap::new();
        let v_sym_name = world.borrow::<View<PropSymName>>().unwrap();
        let v_play_params = world.borrow::<View<PropSchemaPlayParams>>().unwrap();
        for k in id_to_samples.keys() {
            let entity = template_id_to_entity.get(k).unwrap();
            if let Ok(name) = v_sym_name.get(*entity) {
                name_to_schema_id.insert(name.0.to_ascii_lowercase(), *k);
            }
            if let Ok(params) = v_play_params.get(*entity) {
                id_to_play_params.insert(*k, params.clone());
            }
        }

        trace!("{:?}", name_to_schema_id);
        SoundSchema {
            name_to_schema_id,
            id_to_samples,
            id_to_play_params,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_with_params(params: PropSchemaPlayParams) -> SoundSchema {
        SoundSchema {
            name_to_schema_id: HashMap::from([("test".to_owned(), -1)]),
            id_to_samples: HashMap::from([(
                -1,
                vec![SchemaSample {
                    sample_name: "sample".to_owned(),
                    frequency: 1,
                }],
            )]),
            id_to_play_params: HashMap::from([(-1, params)]),
        }
    }

    #[test]
    fn millibels_convert_to_linear_gain_and_channel_pan() {
        let resolved = ResolvedSoundSchema {
            sample_name: "sample".to_owned(),
            volume_millibels: -2500,
            pan_millibels: -1000,
        };
        assert!((resolved.linear_gain() - 0.056_234_132).abs() < 0.000_001);
        let [left, right] = resolved.channel_gains();
        assert_eq!(left, 1.0);
        assert!((right - 0.316_227_76).abs() < 0.000_001);
    }

    #[test]
    fn schema_pan_flags_control_fixed_and_randomized_pan() {
        let fixed = schema_with_params(PropSchemaPlayParams {
            flags: PropSchemaPlayParams::PAN_POSITION,
            pan: -700,
            ..Default::default()
        });
        assert_eq!(fixed.resolve("test").unwrap().pan_millibels, -700);

        let ranged = schema_with_params(PropSchemaPlayParams {
            flags: PropSchemaPlayParams::PAN_RANGE,
            pan: 1200,
            ..Default::default()
        });
        for _ in 0..32 {
            assert!((-1200..=1200).contains(&ranged.resolve("test").unwrap().pan_millibels));
        }

        let unpanned = schema_with_params(PropSchemaPlayParams {
            pan: 9000,
            ..Default::default()
        });
        assert_eq!(unpanned.resolve("test").unwrap().pan_millibels, 0);
    }
}
