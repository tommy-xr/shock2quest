pub mod audio;
mod bitmap_animation;
pub mod font;
pub mod gamesys;
pub mod glb_model;
pub mod glb_skeleton;
pub mod map;
pub mod mission;
pub mod model;
pub mod motion;
pub mod name_map;
pub mod ss2_bin_ai_loader;
pub mod ss2_bin_header;
pub mod ss2_bin_obj_loader;
pub mod ss2_cal_loader;
pub mod ss2_chunk_file_reader;
pub mod ss2_common;
pub mod ss2_entity_info;
pub mod ss2_skeleton;
pub mod tag_database;
pub mod util;

pub mod properties;

pub mod importers;

pub use bitmap_animation::*;
pub use gamesys::*;
pub use name_map::*;
pub use tag_database::*;

// SCALE_FACTOR - the amount to adjust the scale of the world
// The goal of this is to size the world so it feels correct in VR
pub const SCALE_FACTOR: f32 = 2.5;
