use core::fmt;
use std::{collections::HashMap, io};

use rand::{distributions::WeightedIndex, prelude::Distribution, thread_rng};
use shipyard::{Component, Get, IntoIter, IntoWithId, View, World};
use tracing::info;

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

    // Debug output for rendering strings:
    let mut data_to_name = HashMap::new();
    for (k, v) in &sound_schema.id_to_samples {
        let str = v
            .into_iter()
            .map(|sample| sample.sample_name.to_owned())
            .collect::<Vec<String>>()
            .join(",");
        data_to_name.insert(*k, str);
    }

    // println!("sound_schema;:{:#?}", sound_schema);
    // println!("speech db: {:#?}", speech_db);
    speech_db.voices[0].tag_maps[1].debug_print(
        &speech_db.tag_map.index_to_name,
        &data_to_name,
        &speech_db.value_map.index_to_name,
    );
    // println!("env namemap: {:#?}", env_tag_map);
    //panic!();

    Gamesys {
        entity_info,
        sound_schema,
        env_tag_map,
        speech_db,
    }
}

pub fn log_property<T>(world: &World)
where
    T: Component + fmt::Debug + Sync + Clone + Send,
{
    world.run(
        |v_template_id: View<crate::properties::PropTemplateId>,
         v_symname: View<crate::properties::PropSymName>,
         v_property: View<T>| {
            for (id, door) in (&v_property).iter().with_id() {
                let maybe_template_id = v_template_id.get(id);
                let maybe_sym_name = v_symname.get(id);
                println!("({id:?})[{maybe_template_id:?}|{maybe_sym_name:?}] prop: {door:?}")
            }
        },
    );
}
