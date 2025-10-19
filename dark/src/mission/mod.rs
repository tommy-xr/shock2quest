mod bsp_tree;
mod cell;
mod cell_portal;
mod plane;
pub mod render_params;
pub mod room;
pub mod room_database;
pub mod scene_builder;
mod song_params;
pub mod texture_list;

pub use bsp_tree::*;
pub use cell::*;
pub use cell_portal::*;
pub use plane::*;

use crate::properties::LinkDefinitionWithData;

use crate::ss2_chunk_file_reader::ChunkFileTableOfContents;
use crate::ss2_common::read_bytes;
use crate::Gamesys;
use crate::SCALE_FACTOR;
use render_params::*;
use room_database::*;

pub use song_params::*;
use texture_list::*;

use crate::properties::LinkDefinition;
use crate::ss2_common::read_i32;

use crate::importers::TEXTURE_IMPORTER;
use crate::ss2_common::read_u32;
use cgmath::vec4;
use cgmath::Vector4;
use engine::assets::asset_cache::AssetCache;
use engine::scene::VertexPositionTextureLightmapAtlasNormal;
pub use scene_builder::to_scene;

use crate::properties::PropertyDefinition;
use crate::ss2_chunk_file_reader;
use crate::ss2_entity_info;
use crate::ss2_entity_info::SystemShock2EntityInfo;

use engine::texture_atlas::TexturePackResult;

use byteorder::ReadBytesExt;
use cgmath::InnerSpace;
use cgmath::{vec2, vec3, Vector3};
use engine::texture_atlas::TexturePacker;
use std::collections::HashMap;

use std::f32;
use std::io;
use std::io::SeekFrom;

use crate::ss2_common::read_string_with_size;

#[derive(Clone)]
pub struct SystemShock2Geometry {
    pub verts: Vec<VertexPositionTextureLightmapAtlasNormal>,
    pub texture_idx: u16, // Texture index to use

    pub cell_idx: u32, // Index of cell
    pub poly_idx: u32, // Index of poly in cell

    pub lightmap_pack_result: TexturePackResult,
}

pub struct SystemShock2Level {
    pub all_geometry: Vec<SystemShock2Geometry>,

    pub textures: TextureList,
    pub cells: Vec<Cell>,

    pub lightmap_atlas: TexturePacker<image::Rgb<u8>>,

    pub entity_info: SystemShock2EntityInfo,
    pub obj_map: HashMap<i32, String>,

    pub room_database: RoomDatabase,
    pub song_params: SongParams,
    pub bsp_tree: BspTree,
}

impl SystemShock2Level {
    pub fn get_cell_idx_from_position(&self, position: Vector3<f32>) -> Option<u32> {
        self.bsp_tree.cell_from_position(position)
    }
    pub fn get_cell_from_position(&self, position: Vector3<f32>) -> Option<&Cell> {
        let idx = self.get_cell_idx_from_position(position)?;

        self.cells.get(idx as usize)
    }
}

#[derive(Debug)]
pub struct TextureSize {
    pub width: u32,
    pub height: u32,
}

const LIGHTMAP_SIZE: u32 = 4096;

pub struct UVCalculationInfo {
    origin: Vector3<f32>,
    axis_u: Vector3<f32>,
    axis_v: Vector3<f32>,
    mag2_u: f32,
    mag2_v: f32,
    dotp: f32,
    sh_u: f32,
    sh_v: f32,
    rs_x: f32,
    rs_y: f32,
    lsh_u: f32,
    lsh_v: f32,
    lrs_x: f32,
    lrs_y: f32,
}

pub fn read<T: io::Read + io::Seek>(
    asset_cache: &mut AssetCache,
    reader: &mut T,
    gamesys: &Gamesys,
    links: &Vec<Box<dyn LinkDefinition>>,
    links_with_data: &Vec<Box<dyn LinkDefinitionWithData>>,
    properties: &Vec<Box<dyn PropertyDefinition<T>>>,
) -> SystemShock2Level {
    let table_of_contents = ss2_chunk_file_reader::read_table_of_contents(reader);

    let wr_ext = table_of_contents.has_chunk("WREXT".to_string()); // Extended representation
    let wr_rgb = table_of_contents.has_chunk("WRRGB".to_string()); // RGB representation
    let mut light_size = 1;

    let mut world_chunk_name = "WREXT";
    if wr_rgb {
        world_chunk_name = "WRRGB"
    }
    let wr_chunk = table_of_contents
        .get_chunk(world_chunk_name.to_string())
        .unwrap();

    let wr_offset = wr_chunk.offset;

    if wr_rgb {
        light_size = 2
    }

    reader.seek(SeekFrom::Start(wr_offset)).unwrap();

    let _wr_unk = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    // For WR_EXT - load extended attributes
    // Not sure what a bunch of these are - but the light depth is important
    // for us to properly read the light maps
    if wr_ext {
        let _wr_new_dark_unk = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let _wr_shadowed_water = reader.read_u32::<byteorder::LittleEndian>().unwrap();

        let wr_lm_bit_depth = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        light_size = 2u8.pow(wr_lm_bit_depth + 1);

        let _wr_new_dark_unk2 = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let _wr_mysterious_value = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    }

    let wr_num_cells = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    // Read cells
    let mut cells: Vec<Cell> = Vec::new();
    let mut packer = TexturePacker::<image::Rgb<u8>>::new_rgb(LIGHTMAP_SIZE, LIGHTMAP_SIZE);
    for cell_idx in 0..wr_num_cells {
        let cell = Cell::read(reader, &mut packer, wr_ext, cell_idx, light_size);
        cells.push(cell);
    }

    let bsp_tree = BspTree::read(reader, &cells);

    if wr_ext {
        let _ = read_bytes(reader, wr_num_cells as usize);
    }

    let num_static_lights = read_u32(reader);
    let num_dynamic_lights = read_u32(reader);
    tracing::debug!(
        "Mission lights - static: {} dynamic: {}",
        num_static_lights,
        num_dynamic_lights
    );

    let (obj_map, obj_texture_families) = read_obj_map(&table_of_contents, reader);
    let entity_info = ss2_entity_info::new(
        &table_of_contents,
        links,
        links_with_data,
        properties,
        reader,
    );

    let textures = TextureList::read(
        &table_of_contents,
        gamesys,
        &entity_info,
        obj_texture_families,
        reader,
    );
    let all_geometry = create_geometry(asset_cache, &cells, &textures.0);

    let _render_params = RenderParams::read(&table_of_contents, reader);
    let room_database = RoomDatabase::read(&table_of_contents, reader);
    let song_params = SongParams::read(&table_of_contents, reader);

    SystemShock2Level {
        bsp_tree,
        all_geometry,
        textures,
        lightmap_atlas: packer,
        obj_map,
        cells,
        entity_info,
        room_database,
        song_params,
    }
}

fn read_obj_map<T: io::Read + io::Seek>(
    table_of_contents: &ChunkFileTableOfContents,
    reader: &mut T,
) -> (HashMap<i32, String>, Vec<(String, i32)>) {
    let obj_map_chunk = table_of_contents.get_chunk("OBJ_MAP".to_string()).unwrap();
    let len = obj_map_chunk.length;
    reader.seek(SeekFrom::Start(obj_map_chunk.offset)).unwrap();

    let end = obj_map_chunk.offset + len;

    let mut texture_families = Vec::new();

    let mut obj_map = HashMap::new();
    while reader.stream_position().unwrap() < end {
        let obj_id = read_i32(reader);
        let size = read_u32(reader);

        let str = read_string_with_size(reader, size as usize);

        if str.starts_with("t_fam") {
            texture_families.push((str.clone(), obj_id));
        }

        obj_map.insert(obj_id, str);
    }
    (obj_map, texture_families)
}

fn create_geometry(
    asset_cache: &mut AssetCache,
    cells: &Vec<Cell>,
    textures: &Vec<SystemShock2Texture>,
) -> Vec<SystemShock2Geometry> {
    let mut all_geometry: Vec<SystemShock2Geometry> = Vec::new();
    let mut cell_idx = 0;
    for cell in cells {
        let num_render_polys = cell.textured_polygons.len();

        // Create geometry!
        for poly in 0..num_render_polys {
            let render_poly = &cell.textured_polygons[poly];
            let indices = &cell.polygon_indices[poly];
            let li = &cell.lights[poly];
            if li.debug_idx != cell_idx {
                panic!("index doesn't match????");
            }
            let len = indices.len();
            // TODO: What are 249/247 - BACKHACK or something?
            if render_poly.texture_num == 249
                || render_poly.texture_num == 247
                || render_poly.texture_num == 248
            {
                continue;
            }

            let origin = cell.vertices[indices[render_poly.origin_vertex as usize] as usize];

            let axis_u = render_poly.axis_u;
            let axis_v = render_poly.axis_v;

            let mag2_u = axis_u.magnitude2();
            let mag2_v = axis_v.magnitude2();

            let dotp = axis_u.dot(axis_v);

            let sh_u = render_poly.u / 4096.0;
            let sh_v = render_poly.v / 4096.0;

            let tex_info = &textures[render_poly.texture_num as usize];
            let texture_dim = texture_dimensions(asset_cache, tex_info);

            let rs_x = (texture_dim.width as f32) / 64.0;
            let rs_y = (texture_dim.height as f32) / 64.0;

            let lsh_u = (0.5 - li.u as f32) + (render_poly.u / 1024.0);
            let lsh_v = (0.5 - li.v as f32) + (render_poly.v / 1024.0);

            let lrs_x = li.lx as f32;
            let lrs_y = li.ly as f32;

            let uv_calc_info = UVCalculationInfo {
                origin,
                axis_u,
                axis_v,
                dotp,
                rs_x,
                rs_y,
                sh_u,
                sh_v,
                mag2_u,
                mag2_v,
                lsh_u,
                lsh_v,
                lrs_x,
                lrs_y,
            };

            let mut verts = Vec::new();
            for idx in 1..(len - 1) {
                verts.push(build_vertex(
                    cell.vertices[indices[idx] as usize],
                    &uv_calc_info,
                    &li.texture_pack_result,
                ));
                verts.push(build_vertex(
                    cell.vertices[indices[idx + 1] as usize],
                    &uv_calc_info,
                    &li.texture_pack_result,
                ));
                verts.push(build_vertex(
                    cell.vertices[indices[0] as usize],
                    &uv_calc_info,
                    &li.texture_pack_result,
                ));
            }

            all_geometry.push(SystemShock2Geometry {
                verts,
                cell_idx,
                poly_idx: poly as u32,
                texture_idx: render_poly.texture_num,
                lightmap_pack_result: li.texture_pack_result,
            });
        }
        cell_idx += 1;
    }
    all_geometry
}

fn build_vertex(
    vec: Vector3<f32>,
    uv_calc_info: &UVCalculationInfo,
    atlas_info: &TexturePackResult,
) -> VertexPositionTextureLightmapAtlasNormal {
    let lightmap_u;
    let lightmap_v;
    let tex_u;
    let tex_v;
    let ax_u = uv_calc_info.axis_u;
    let ax_v = uv_calc_info.axis_v;

    // Calculate face normal from cross product of texture axes
    // Try inverted normal - Dark Engine might use different winding order
    let face_normal = -(ax_u.cross(ax_v).normalize());
    let mag2_u = uv_calc_info.mag2_u;
    let mag2_v = uv_calc_info.mag2_v;
    let dotp = uv_calc_info.dotp;
    let sh_u = uv_calc_info.sh_u;
    let sh_v = uv_calc_info.sh_v;
    let rs_x = uv_calc_info.rs_x;
    let rs_y = uv_calc_info.rs_y;
    let lsh_u = uv_calc_info.lsh_u;
    let lsh_v = uv_calc_info.lsh_v;
    let lrs_x = uv_calc_info.lrs_x;
    let lrs_y = uv_calc_info.lrs_y;
    let lm_scale = 4.0;
    if uv_calc_info.dotp.abs() < 0.0001 {
        let vrelative = vec - uv_calc_info.origin;
        let projected = vec2(ax_u.dot(vrelative) / mag2_u, ax_v.dot(vrelative) / mag2_v);
        // Textured u/v
        tex_u = (projected.x + uv_calc_info.sh_u) / uv_calc_info.rs_x;
        tex_v = (projected.y + uv_calc_info.sh_v) / uv_calc_info.rs_y;

        // lightmap u/v
        lightmap_u = (lm_scale * projected.x + lsh_u) / lrs_x;
        lightmap_v = (lm_scale * projected.y + lsh_v) / lrs_y;
    } else {
        // Texture axes not orthogonal... a more complicated case
        let corr = 1.0 / (mag2_u * mag2_v - dotp * dotp);
        let cu = corr * mag2_v;
        let cv = corr * mag2_u;
        let cross = corr * dotp;

        let vrelative = vec - uv_calc_info.origin;
        let pr = vec2(ax_u.dot(vrelative), ax_v.dot(vrelative));
        let projected = vec2(pr.x * cu - pr.y * cross, pr.y * cv - pr.x * cross);
        // Textured u/v
        tex_u = (projected.x + sh_u) / rs_x;
        tex_v = (projected.y + sh_v) / rs_y;

        // lightmap u/v
        lightmap_u = (lm_scale * projected.x + lsh_u) / lrs_x;
        lightmap_v = (lm_scale * projected.y + lsh_v) / lrs_y;
    }

    let lightmap_atlas = vec4(
        atlas_info.uv_offset_x,
        atlas_info.uv_offset_y,
        atlas_info.uv_width,
        atlas_info.uv_height,
    );

    vert(
        vec.x,
        vec.y,
        vec.z,
        tex_u,
        tex_v,
        lightmap_u,
        lightmap_v,
        lightmap_atlas,
        face_normal,
    )
}

fn vert(
    x: f32,
    y: f32,
    z: f32,
    u: f32,
    v: f32,
    lightmap_u: f32,
    lightmap_v: f32,
    atlas_info: Vector4<f32>,
    normal: Vector3<f32>,
) -> VertexPositionTextureLightmapAtlasNormal {
    VertexPositionTextureLightmapAtlasNormal {
        position: vec3(x, y, z) / SCALE_FACTOR,
        uv: vec2(u, v),
        lightmap_uv: vec2(lightmap_u, lightmap_v),
        lightmap_atlas: atlas_info,
        normal,
    }
}

fn texture_dimensions(asset_cache: &mut AssetCache, tex_info: &SystemShock2Texture) -> TextureSize {
    if tex_info.texture_filename == "null" {
        TextureSize {
            width: 1,
            height: 1,
        }
    } else {
        let tex_name = format!(
            "{}/{}.PCX",
            tex_info.family.to_uppercase(),
            tex_info.texture_filename
        );
        let texture = asset_cache.get(&TEXTURE_IMPORTER, &tex_name);

        TextureSize {
            width: texture.width(),
            height: texture.height(),
        }
    }
}
