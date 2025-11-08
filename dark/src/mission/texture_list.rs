use crate::Gamesys;
use crate::properties::{AnimTexFlags, PropAnimTex, PropRenderType, RenderType};
use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
use crate::ss2_entity_info::{self, SystemShock2EntityInfo};
use byteorder::ReadBytesExt;
use shipyard::{Get, View, World};
use tracing::{info, warn};

use std::collections::{HashMap, HashSet};
use std::io;
use std::io::SeekFrom;

use crate::ss2_common::read_string_with_size;

#[derive(Clone, Debug)]
pub struct TextureAnimationInfo {
    pub rate_in_milliseconds: u32,
    pub flags: AnimTexFlags,
}

#[derive(Debug)]
pub struct SystemShock2Texture {
    pub family: String,
    pub texture_filename: String,
    pub render_type: RenderType,
    pub animation_info: Option<TextureAnimationInfo>,
}

pub struct TextureList(pub Vec<SystemShock2Texture>);

impl TextureList {
    // Read the TXLIST chunk to get a list of the textures used
    pub fn read<T: io::Read + io::Seek>(
        table_of_contents: &ChunkFileTableOfContents,
        gamesys: &Gamesys,
        entity_info: &SystemShock2EntityInfo,
        obj_texture_families: Vec<(String, i32)>,
        reader: &mut T,
    ) -> TextureList {
        // First, let's prepare by reading the texture archetypes from the world definition
        // This give us information that is stored in archetypes, like Render Type (ie, RenderType 2 is FullBright/Unlit)
        let name_to_info = read_texture_archetypes(obj_texture_families, entity_info, gamesys);

        // Finally, once we have the archetype data, we can use it to build the final result list
        read_txlist_chunk(table_of_contents, reader, name_to_info)
    }
}

fn read_txlist_chunk<T: io::Read + io::Seek>(
    table_of_contents: &ChunkFileTableOfContents,
    reader: &mut T,
    name_to_info: HashMap<String, (RenderType, Option<TextureAnimationInfo>)>,
) -> TextureList {
    let txlist = table_of_contents
        .get_chunk("TXLIST".to_string())
        .unwrap()
        .offset;
    reader.seek(SeekFrom::Start(txlist)).unwrap();

    let _txt_length = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let txt_count = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let fam_count = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    let mut texture_fams = Vec::new();
    let mut textures = Vec::new();

    // Texture families are top-level folders in the res/fam zip file,
    // and share the same palette (which is of no consequence here...)
    for _ in 0..fam_count {
        let fam = read_string_with_size(reader, 16);
        texture_fams.push(fam);
    }

    for _ in 0..txt_count {
        let _one = reader.read_u8().unwrap();
        let fam = reader.read_u8().unwrap();
        let _zero = reader.read_u16::<byteorder::LittleEndian>().unwrap();
        let name = read_string_with_size(reader, 16);

        let mut family = "".to_owned();
        if fam > 0 && fam <= (texture_fams.len() as u8) {
            family = texture_fams[(fam - 1) as usize].to_owned();
        }

        let entity_name = format!("t_fam/{}/{}", family, name);

        let (render_type, maybe_animation_info) = {
            if let Some(info) = name_to_info.get(&entity_name) {
                info!("texture info for: {} is {:?}", entity_name, info);
                (info.0.clone(), info.1.clone())
            } else {
                warn!("no texture info for: {}", entity_name);
                (RenderType::Normal, None)
            }
        };

        textures.push(SystemShock2Texture {
            family,
            texture_filename: name,
            render_type,
            animation_info: maybe_animation_info,
        })
    }
    TextureList(textures)
}

fn read_texture_archetypes(
    obj_texture_families: Vec<(String, i32)>,
    entity_info: &SystemShock2EntityInfo,
    gamesys: &Gamesys,
) -> HashMap<String, (RenderType, Option<TextureAnimationInfo>)> {
    let mut world = World::new();
    let name_map_override = HashMap::new();

    let mut id_to_include = HashSet::new();
    let mut name_to_id = HashMap::new();
    let mut name_to_info = HashMap::new();
    for (family_name, id) in &obj_texture_families {
        id_to_include.insert(id);
        name_to_id.insert(family_name, id);
    }

    let merged_entity_info = ss2_entity_info::merge_with_gamesys(entity_info, gamesys);

    let template_to_entity_id =
        merged_entity_info.initialize_world_with_entities(&mut world, name_map_override, |id| {
            id_to_include.contains(&id)
        });
    for (family_name, id) in &obj_texture_families {
        let v_render_type = world.borrow::<View<PropRenderType>>().unwrap();
        let v_anim_tex = world.borrow::<View<PropAnimTex>>().unwrap();

        let maybe_entity_id = template_to_entity_id.get(id);
        if let Some(entity_id) = maybe_entity_id {
            let maybe_render_type = v_render_type.get(*entity_id);
            let maybe_anim_tex = v_anim_tex.get(*entity_id);
            let render_type = {
                if let Ok(render_type) = maybe_render_type {
                    render_type.0.clone()
                } else {
                    RenderType::Normal
                }
            };

            let maybe_texture_animation_info = {
                if let Ok(anim_tex) = maybe_anim_tex {
                    Some(TextureAnimationInfo {
                        rate_in_milliseconds: anim_tex.rate_in_milliseconds,
                        flags: anim_tex.anim_flags.clone(),
                    })
                } else {
                    None
                }
            };
            name_to_info.insert(
                family_name.clone(),
                (render_type, maybe_texture_animation_info),
            );

            // if let Ok(anim_tex) = maybe_anim_tex {
            //     // TODO:
            //     panic!("got an animated texture!");
            // }
        }
    }
    name_to_info
}
