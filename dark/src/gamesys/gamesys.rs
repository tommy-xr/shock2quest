use std::io;

use crate::{
    EnvMap, EnvSoundQuery, ResolvedSoundSchema, SoundSchema, SpeechDB, TagDatabase,
    gamesys::params::{HrmParams, SkillParams, TrainerCostTables},
    properties::{LinkDefinition, LinkDefinitionWithData, PropertyDefinition},
    ss2_chunk_file_reader::{self},
    ss2_entity_info::{self, SystemShock2EntityInfo},
};

pub struct Gamesys {
    pub sound_schema: SoundSchema,
    pub entity_info: SystemShock2EntityInfo,
    env_tag_map: TagDatabase,
    speech_db: SpeechDB,
    /// Trainer upgrade cost tables (`STATCOST`/`WTECHCOST`/`WSKILLCOST`/
    /// `PSICOST` file-var chunks); `None` if the gamesys lacks them.
    trainer_costs: Option<TrainerCostTables>,
    /// HRM hacking/repair/modify tuning (`HRM` file-var chunk).
    hrm_params: Option<HrmParams>,
    /// Skill-system tuning (`SKILLPARAM` file-var chunk).
    skill_params: Option<SkillParams>,
}

impl Gamesys {
    pub fn get_random_environmental_sound(
        &self,
        query: &EnvSoundQuery,
    ) -> Option<ResolvedSoundSchema> {
        let tag_query = query.to_tag_query(&self.speech_db.tag_map, &self.speech_db.value_map);
        let result = self.env_tag_map.query_match_all(&tag_query);
        // `query_match_all` appends the data of every node it matched, in
        // depth-first order, so the shallowest match on the path comes first
        // and the deepest-visited one last. With a fully-specified query that
        // descends a single path - which is what `most_specific` is for - the
        // last id is that path's refinement; it is NOT a general "most
        // specific" selector across several matching sibling branches.
        let id = if query.prefers_most_specific() {
            result.last()
        } else {
            result.first()
        }?;

        self.sound_schema.resolve_id(*id)
    }

    pub fn speech_db(&self) -> &SpeechDB {
        &self.speech_db
    }

    pub fn sound_schema(&self) -> &SoundSchema {
        &self.sound_schema
    }

    pub fn entity_info(&self) -> &SystemShock2EntityInfo {
        &self.entity_info
    }

    pub fn into_entity_info(self) -> SystemShock2EntityInfo {
        self.entity_info
    }

    /// The trainer upgrade cost tables, if this gamesys carries them.
    pub fn trainer_costs(&self) -> Option<&TrainerCostTables> {
        self.trainer_costs.as_ref()
    }

    pub fn hrm_params(&self) -> Option<&HrmParams> {
        self.hrm_params.as_ref()
    }

    pub fn skill_params(&self) -> Option<&SkillParams> {
        self.skill_params.as_ref()
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
    let trainer_costs = TrainerCostTables::read(&table_of_contents, reader);
    let hrm_params = HrmParams::read(&table_of_contents, reader);
    let skill_params = SkillParams::read(&table_of_contents, reader);

    // Uncomment to output debug info for voices:
    // debug_print_voices(&sound_schema, &speech_db);
    // panic!()

    Gamesys {
        entity_info,
        sound_schema,
        env_tag_map,
        speech_db,
        trainer_costs,
        hrm_params,
        skill_params,
    }
}
