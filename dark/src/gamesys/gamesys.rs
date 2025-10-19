use std::{io};

use rand::{distributions::WeightedIndex, prelude::Distribution, thread_rng};

use crate::{
    properties::{LinkDefinition, LinkDefinitionWithData, PropertyDefinition},
    ss2_chunk_file_reader::{self},
    ss2_entity_info::{self, SystemShock2EntityInfo},
    EnvMap, EnvSoundQuery, SoundSchema, SpeechDB, TagDatabase,
};

pub struct Gamesys {
    pub sound_schema: SoundSchema,
    pub entity_info: SystemShock2EntityInfo,
    env_tag_map: TagDatabase,
    speech_db: SpeechDB,
}

impl Gamesys {
    pub fn get_random_environmental_sound(&self, query: &EnvSoundQuery) -> Option<String> {
        let tag_query = query.to_tag_query(&self.speech_db.tag_map, &self.speech_db.value_map);
        let result = self.env_tag_map.query_match_all(&tag_query);

        if result.is_empty() {
            return None;
        }

        let sample = result[0];
        let maybe_samples = self.sound_schema.id_to_samples.get(&sample);

        maybe_samples?;

        let samples = maybe_samples.unwrap();

        let mut rng = thread_rng();
        let weights = samples.iter().map(|s| s.frequency).collect::<Vec<u8>>();
        let weight_index = WeightedIndex::new(weights).unwrap();
        let idx = weight_index.sample(&mut rng);

        Some(samples[idx].sample_name.to_owned())
    }
}

pub fn read<T: io::Read + io::Seek>(
    reader: &mut T,
    links: &Vec<Box<dyn LinkDefinition>>,
    links_with_data: &Vec<Box<dyn LinkDefinitionWithData>>,
    properties: &Vec<Box<dyn PropertyDefinition<T>>>,
) -> Gamesys {
    let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(reader);

    let entity_info = ss2_entity_info::new(
        &table_of_contents,
        links,
        links_with_data,
        properties,
        reader,
    );

    let sound_schema = SoundSchema::read(&table_of_contents, reader, &entity_info);

    let env_tag_map = EnvMap::read(&table_of_contents, reader);
    let speech_db = SpeechDB::read(&table_of_contents, reader);

    // Uncomment to output debug info for voices:
    // debug_print_voices(&sound_schema, &speech_db);
    // panic!()

    Gamesys {
        entity_info,
        sound_schema,
        env_tag_map,
        speech_db,
    }
}

