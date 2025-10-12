mod animation_clip;
mod animation_player;
mod motion_clip;
mod motion_info;
pub mod motion_query;
mod motion_schema;

pub use animation_clip::*;
pub use animation_player::*;
pub use motion_clip::*;
pub use motion_info::*;
pub use motion_query::*;
pub use motion_schema::*;
use rand::{thread_rng, Rng};

use crate::{
    ss2_chunk_file_reader,
    ss2_common::{
        self, read_bool, read_bytes, read_i32, read_single, read_string_with_size, read_u32,
        read_u8,
    },
    NameMap, TagDatabase, SCALE_FACTOR,
};
use cgmath::{Deg, Vector3};

use std::{collections::HashMap, io};
use tracing::{info, trace};

pub struct MotionDB {
    animation_name_to_index: HashMap<String, u32>,
    mps_motions: Vec<MpsMotion>,
    motion_stuffs: Vec<MotionStuff>,
    tag_databases: Vec<TagDatabase>,
    // Dictionary to resolve tags -> indices for querying
    tag_name_map: NameMap,

    // Dictionary to resolve values -> strings for animation names
    tag_value_to_animations: HashMap<i32, Vec<String>>,
}

impl MotionDB {
    pub fn get_mps_motions(&self, name: String) -> &MpsMotion {
        let lowercase_name = &name.to_ascii_lowercase();
        let idx = self.animation_name_to_index.get(lowercase_name).unwrap();
        &self.mps_motions[*idx as usize]
    }

    pub fn get_motion_stuff(&self, name: String) -> &MotionStuff {
        let lowercase_name = &name.to_ascii_lowercase();
        let idx = self.animation_name_to_index.get(lowercase_name).unwrap();
        &self.motion_stuffs[*idx as usize]
    }
}

#[derive(Debug)]
pub struct MotionStuff {
    pub flags: u32,
    pub blend_length: u16,
    pub end_direction: Deg<f32>,
    pub translation: Vector3<f32>,
    pub duration: f32,
}

impl MotionDB {
    pub fn read<T: io::Read + io::Seek>(reader: &mut T) -> MotionDB {
        let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(reader);

        let mot_chunk = table_of_contents.get_chunk("MotDBase".to_owned()).unwrap();

        reader.seek(io::SeekFrom::Start(mot_chunk.offset));

        // Load Namemap
        let (animation_name_to_index, index_to_animation_name) = load_name_map(reader);

        // Read motstuff
        let motstuff_size = ss2_common::read_u32(reader);
        trace!("motstuff_size: {motstuff_size}");

        let mut motion_stuffs = Vec::new();
        for _i in 0..motstuff_size {
            let motstuff = read_motion_stuff(reader);
            motion_stuffs.push(motstuff);
        }

        let mps_motion_size = ss2_common::read_u32(reader);

        trace!("all sizes. motstuff_size: {motstuff_size} mps_motion_size: {mps_motion_size}");

        // Read mps motions
        let mut mps_motions = Vec::new();
        for _i in 0..mps_motion_size {
            let mps_motion = read_mps_motion(reader);
            mps_motions.push(mps_motion);
        }

        // Load tags
        let _name_map = NameMap::read(reader);
        let name_map = NameMap::read(reader);
        //let (tag_map, key_to_tag_name) = load_name_map(reader);
        //trace!("animation name map: {:#?}", animation_name_to_index);
        //trace!("name map: {:#?}", name_map);
        //trace!("tag map: {:#?}", tag_map);

        let num_actors = read_u32(reader);
        trace!("num actors: {}", num_actors);
        let num_tag_sets = read_u32(reader);

        trace!("num_tag_sets: {num_tag_sets}");

        for _ in 0..num_tag_sets {
            let _is_mandatory = read_bool(reader);
            let _weight = read_single(reader);
        }

        // // load tag databases
        let n_cat = read_u32(reader);
        trace!("ncat: {n_cat}");

        let mut tag_databases = Vec::new();
        for _i in 0..n_cat {
            let tag_database = TagDatabase::read(reader);
            tag_databases.push(tag_database);
        }

        let mut tag_value_to_animation_name = HashMap::new();
        let mut tag_value_to_animations = HashMap::new();
        let mut tag_value_to_motion_schema = HashMap::new();
        let schemas = read_u32(reader);
        trace!("motion_schema_count: {schemas}");
        for i in 0..schemas {
            let schema = MotionSchema::read(reader);
            let animations = schema
                .motion_index_list
                .iter()
                .map(|id| {
                    let str = index_to_animation_name.get(id).unwrap();
                    str.to_owned()
                })
                .collect::<Vec<String>>();

            let summary_str = schema
                .motion_index_list
                .iter()
                .map(|id| {
                    let str = index_to_animation_name.get(id).unwrap();
                    str.to_owned()
                })
                .collect::<Vec<String>>()
                .join(", ");

            tag_value_to_animation_name.insert(i as i32, summary_str);
            tag_value_to_motion_schema.insert(i as i32, schema);
            tag_value_to_animations.insert(i as i32, animations);
        }

        // GENERATION: Output 'friendly' version of animation db
        // tag_databases[0].debug_print(
        //     &key_to_tag_name,
        //     &tag_value_to_animation_name,
        //     &HashMap::new(), /* no enum values */
        // );

        MotionDB {
            animation_name_to_index,
            mps_motions,
            motion_stuffs,
            tag_databases,
            tag_name_map: name_map,
            tag_value_to_animations,
        }
    }
    ///
    /// query the motion database
    ///
    /// Returns a string containing the name of the animation
    pub fn query(&self, query: MotionQuery) -> Option<String> {
        info!("motion_query: {:?}", query);
        let creature_type = query.creature_type;

        if creature_type >= self.tag_databases.len() as u32 {
            return None;
        }

        let tag_database = &self.tag_databases[creature_type as usize];

        // println!("name_map: {:?}", &self.tag_name_map);
        let tag_query = query.to_tag_query(&self.tag_name_map);

        info!("tag_query: {:?}", tag_query);

        let query_result = tag_database.query_match_all(&tag_query);
        info!("query_result: {:?}", tag_query);

        if query_result.is_empty() {
            return None;
        }

        let options = query_result
            .into_iter()
            .filter_map(|idx| self.tag_value_to_animations.get(&idx))
            .flatten()
            .collect::<Vec<&String>>();

        info!("options: {:?}", options);

        match query.selection_strategy {
            MotionQuerySelectionStrategy::Random => {
                let mut rng = thread_rng();
                let idx = rng.gen_range(0..options.len());

                let opt = options[idx];
                info!("querying - got: {}", opt);
                Some(opt.to_owned())
            }
            MotionQuerySelectionStrategy::Sequential(seq) => {
                let idx = (seq as usize) % options.len();
                let opt = options[idx];
                return Some(opt.to_owned());
            }
        }
    }
}

fn load_name_map<T: io::Read + io::Seek>(
    reader: &mut T,
) -> (HashMap<String, u32>, HashMap<u32, String>) {
    let _upper_bound = ss2_common::read_i32(reader);
    let _lower_bound = ss2_common::read_i32(reader);
    let size = ss2_common::read_u32(reader);

    let mut animation_name_to_index = HashMap::new();
    let mut index_to_animation_name = HashMap::new();

    // TODO: What is the name map used for?
    for i in 0..size {
        let char = ss2_common::read_char(reader);

        if char == '+' {
            let name = ss2_common::read_string_with_size(reader, 16);
            animation_name_to_index.insert(name.to_ascii_lowercase().to_owned(), i);
            index_to_animation_name.insert(i, name.to_ascii_lowercase().to_owned());
        }
    }
    (animation_name_to_index, index_to_animation_name)
}

pub type JointId = u32;

#[derive(Debug)]
pub struct MotionComponent {
    motion_type: i32,
    joint_id: JointId,
    handle: u32,
}

use bitflags::bitflags;

bitflags! {
    pub struct MotionFlags: u32 {
        const STANDING = 1 << 0;
        const LEFT_FOOT_STEP = 1 << 1;
        const RIGHT_FOOT_STEP = 1 << 2;
        const LEFT_FOOT_UP = 1 << 3;
        const RIGHT_FOOT_UP = 1 << 4;
        const FIRE = 1 << 5;
        const INTERRUPTIBLE = 1 << 6;
        const START = 1 << 7;
        const END = 1 << 8;

        // What are these?
        const UNK1 = 1 << 9;
        const UNK2 = 1 << 10;
        const UNK3 = 1 << 11;
        const UNK4 = 1 << 12;
        const UNK5 = 1 << 13;
        const UNK6 = 1 << 14;
        const UNK7 = 1 << 15;
        const UNK8 = 1 << 16;
    }
}

#[derive(Debug, Clone)]
pub struct FrameFlags {
    pub frame: u32,
    pub flags: MotionFlags,
}

pub struct MpsMotion {
    pub motion_type: u32, //mocap or virtual
    pub motion_components: Vec<MotionComponent>,
    pub motion_flags: Vec<FrameFlags>,
    pub sig: u32, // bitfield for the joints that this animation impacts
    pub frame_count: f32,
    pub frame_rate: i32,
    pub mot_num: i32, // maybe index into motion db
    pub name: String,
}

impl MpsMotion {
    pub fn get_joint_id(&self, id: u32) -> JointId {
        let motion_component = &self.motion_components[id as usize];
        motion_component.joint_id
    }
}

fn read_mps_motion<T: io::Read + io::Seek>(reader: &mut T) -> MpsMotion {
    // Motion Info
    let motion_type = read_u32(reader);
    let sig = read_u32(reader);
    let frame_count = read_single(reader);
    let frame_rate = read_i32(reader);
    let mot_num = read_i32(reader);
    let name = read_string_with_size(reader, 12);
    let _app_type = read_u8(reader);
    let _app_data = read_bytes(reader, 63);

    let num_components = read_i32(reader);
    let _unk1 = read_i32(reader);
    let num_flags = read_i32(reader);
    let _unk2 = read_i32(reader);

    let mut motion_components = Vec::new();
    for _i in 0..num_components {
        let motion_type = read_i32(reader);
        let joint_id = read_u32(reader) as JointId;
        let handle = read_u32(reader);
        let motion_component = MotionComponent {
            motion_type,
            joint_id,
            handle,
        };
        motion_components.push(motion_component);
    }

    let mut motion_flags = Vec::new();
    for _i in 0..num_flags {
        let frame = read_u32(reader);
        let flag_u32 = read_u32(reader);
        let flags = MotionFlags::from_bits(flag_u32).unwrap();
        let motion_flag = FrameFlags { frame, flags };
        motion_flags.push(motion_flag);
    }

    MpsMotion {
        motion_type,
        motion_components,
        sig,
        frame_count,
        frame_rate,
        mot_num,
        name,
        motion_flags,
    }
}

pub fn read_motion_stuff<T: io::Read + io::Seek>(reader: &mut T) -> MotionStuff {
    let flags = ss2_common::read_u32(reader);
    let blend_length = ss2_common::read_u16(reader);
    let end_direction = ss2_common::read_u16_angle(reader);
    let translation = ss2_common::read_vec3(reader) / SCALE_FACTOR;

    // Have to correct the transform - it seems the animation translation is in a different coordinate space
    // then the model?
    // let translation = vec3(
    //     initial_translation.z,
    //     initial_translation.y,
    //     -initial_translation.x,
    // );
    let duration = ss2_common::read_single(reader);

    MotionStuff {
        flags,
        blend_length,
        end_direction,
        translation,
        duration,
    }
}
