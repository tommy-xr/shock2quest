use cgmath::point3;
use engine::scene::SceneObject;
use engine::scene::VertexPosition;
///
/// wr_cell.rs
///
/// WorldRep_Cell contains the data for a single cell in the world representation.
use engine::texture_atlas::TexturePackResult;

use byteorder::ReadBytesExt;
use cgmath::{Vector3, vec3};
use engine::texture_atlas::TexturePacker;

use std::f32;
use std::io;

use crate::SCALE_FACTOR;
use crate::ss2_common::read_vec3;

use super::CellPortal;
use super::Plane;

#[derive(Debug, Clone)]
pub struct Cell {
    pub idx: u32,
    pub center: Vector3<f32>,
    pub radius: f32,
    pub portal_count: u8,
    pub portals: Vec<CellPortal>,
    pub polygons: Vec<Polygon>,
    pub textured_polygons: Vec<PolygonTexturing>,
    pub polygon_indices: Vec<Vec<u8>>,
    pub planes: Vec<Plane>,
    pub vertices: Vec<Vector3<f32>>,
    pub lights: Vec<LightInfo>,
    /// Indices into the mission's object-light table naming the lights that
    /// reach this cell - the candidate set for lighting any object standing
    /// here. Authored at level-build time, so it is not derivable from the
    /// lights' radii.
    pub light_indices: Vec<u16>,
}

impl Cell {
    pub fn read<T: io::Read>(
        reader: &mut T,
        packer: &mut TexturePacker<image::Rgb<u8>>,
        wr_ext: bool,
        cell_idx: u32,
        light_size: u8,
    ) -> Cell {
        let cell_num_verts = reader.read_u8().unwrap();
        let cell_num_polys = reader.read_u8().unwrap();
        let cell_num_render_polys = reader.read_u8().unwrap();
        let portal_count = reader.read_u8().unwrap();
        let cell_num_planes = reader.read_u8().unwrap();
        let _cell_medium = reader.read_u8().unwrap();
        let _cell_flags = reader.read_u8().unwrap();

        let _nxn = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let _poly_map_size = reader.read_u16::<byteorder::LittleEndian>().unwrap();

        let cell_num_anim_lights = reader.read_u8().unwrap();
        let _cell_flow_group = reader.read_u8().unwrap();

        let center = read_vec3(reader) / SCALE_FACTOR;
        let radius = reader.read_f32::<byteorder::LittleEndian>().unwrap() / SCALE_FACTOR;

        let mut vertices = vec![vec3(0.0, 0.0, 0.0); cell_num_verts as usize];

        for v in 0..cell_num_verts {
            vertices[v as usize] = read_vec3(reader);
        }

        let mut polygons: Vec<Polygon> = Vec::new();
        for _ in 0..cell_num_polys {
            let poly = read_polygon(reader);
            polygons.push(poly);
        }

        let mut textured_polygons: Vec<PolygonTexturing> = Vec::new();
        for _ in 0..cell_num_render_polys {
            let textured_poly = read_polygon_texturing(reader, wr_ext);
            textured_polygons.push(textured_poly);
        }

        let _num_indices = reader.read_u32::<byteorder::LittleEndian>().unwrap();
        let mut polygon_indices: Vec<Vec<u8>> = Vec::new();

        for poly in 0..cell_num_polys {
            let p = &polygons[poly as usize];
            let count = p.count;

            let mut indices: Vec<u8> = Vec::new();
            for _i in 0..count {
                let idx = reader.read_u8().unwrap();
                indices.push(idx);
            }
            polygon_indices.push(indices);
        }

        let mut planes: Vec<Plane> = Vec::new();
        for _ in 0..cell_num_planes {
            let plane = Plane::read(reader);
            planes.push(plane);
        }

        let (lights, light_indices) = read_lights(
            packer,
            cell_idx,
            reader,
            cell_num_anim_lights,
            cell_num_render_polys,
            light_size,
        );

        let portals = Self::collect_portals(&polygons, &polygon_indices, &vertices, portal_count);

        let cell = Cell {
            idx: cell_idx,
            portal_count,
            portals,
            center,
            radius,
            polygons,
            textured_polygons,
            polygon_indices,
            planes,
            vertices,
            lights,
            light_indices,
        };
        cell
    }

    pub fn debug_render(&self) -> Vec<SceneObject> {
        let mut ret = Vec::new();

        let mut green_lines: Vec<Vector3<f32>> = Vec::new();
        let mut lines: Vec<Vector3<f32>> = Vec::new();

        lines.push(self.center);
        lines.push(self.center + vec3(0.0, self.radius, 0.0));

        lines.push(self.center);
        lines.push(self.center + vec3(0.0, -self.radius, 0.0));

        let mut i = 0;
        let portal_start = self.polygons.len() - self.portal_count as usize;
        for poly in &self.polygons {
            let is_portal = i >= portal_start;
            // if i < portal_start {
            //     i += 1;
            //     continue;
            // }
            let indices = &self.polygon_indices[i];
            if is_portal {
                // println!(
                //     "i: {} poly count: {} to: {}",
                //     i, poly.count, poly.target_cell
                // );
            }

            for inner_idx in 1..(poly.count - 1) {
                if inner_idx + 1 >= poly.count {
                    break;
                }
                let i0 = indices[0];
                let v0 = self.vertices[i0 as usize] / SCALE_FACTOR;
                let i1 = indices[(inner_idx + 0) as usize];
                let v1 = self.vertices[(i1 + 0u8) as usize] / SCALE_FACTOR;
                let i2 = indices[(inner_idx + 1) as usize];
                let v2 = self.vertices[i2 as usize] / SCALE_FACTOR;
                if is_portal {
                    lines.push(v0);
                    lines.push(v1);

                    lines.push(v0);
                    lines.push(v2);

                    lines.push(v1);
                    lines.push(v2);
                } else {
                    green_lines.push(v0);
                    green_lines.push(v1);

                    green_lines.push(v0);
                    green_lines.push(v2);

                    green_lines.push(v1);
                    green_lines.push(v2);
                }
            }

            i += 1;
        }
        //panic!();

        let line_vertices = lines
            .iter()
            .map(|v| VertexPosition { position: *v })
            .collect();

        let green_line_vertices = green_lines
            .iter()
            .map(|v| VertexPosition { position: *v })
            .collect();

        let green_lines_mat = engine::scene::color_material::create(Vector3::new(1.0, 0.0, 0.0));
        let debug = SceneObject::new(
            green_lines_mat,
            Box::new(engine::scene::lines_mesh::create(green_line_vertices)),
        );

        let aqua_lints_mat = engine::scene::color_material::create(Vector3::new(0.0, 1.0, 1.0));
        let debug2 = SceneObject::new(
            aqua_lints_mat,
            Box::new(engine::scene::lines_mesh::create(line_vertices)),
        );
        ret.push(debug);
        ret.push(debug2);
        ret
    }

    fn collect_portals(
        polygons: &Vec<Polygon>,
        polygon_indices: &Vec<Vec<u8>>,
        vertices: &Vec<Vector3<f32>>,
        portal_count: u8,
    ) -> Vec<CellPortal> {
        let mut portals: Vec<CellPortal> = Vec::new();
        let portal_start = polygons.len() - portal_count as usize;
        for i in portal_start..polygons.len() {
            let poly = &polygons[i];

            let indices = &polygon_indices[i];

            let mut portal_vertices = Vec::new();
            for idx in 0..poly.count {
                let i0 = indices[idx as usize];
                let vertex = vertices[i0 as usize] / SCALE_FACTOR;
                portal_vertices.push(point3(vertex.x, vertex.y, vertex.z));
            }

            let portal = CellPortal::new(portal_vertices, poly.target_cell);
            portals.push(portal);
        }
        portals
    }
}

#[derive(Debug, Clone)]
pub struct Polygon {
    pub flags: u8,
    pub count: u8,
    pub plane_id: u8,
    pub clut_id: u8,
    pub target_cell: u16,
    pub motion_index: u8,
    pub unk: u8,
}

fn read_polygon<T: io::Read>(reader: &mut T) -> Polygon {
    let flags = reader.read_u8().unwrap();
    let count = reader.read_u8().unwrap();
    let plane_id = reader.read_u8().unwrap();
    let clut_id = reader.read_u8().unwrap();
    let target_cell = reader.read_u16::<byteorder::LittleEndian>().unwrap();
    let motion_index = reader.read_u8().unwrap();
    let unk = reader.read_u8().unwrap();

    Polygon {
        flags,
        count,
        plane_id,
        clut_id,
        target_cell,
        motion_index,
        unk,
    }
}

#[derive(Debug, Clone)]
pub struct PolygonTexturing {
    pub axis_u: Vector3<f32>,
    pub axis_v: Vector3<f32>,
    pub u: f32,
    pub v: f32,
    pub texture_num: u16,    // Index into txture list
    pub origin_vertex: u16,  // Vertex index of the origin texture
    pub cached_surface: u16, // Not sure, there is a texture cache, I guess?
    pub scale: f32,
    pub center: Vector3<f32>,
}

fn read_polygon_texturing<T: io::Read>(reader: &mut T, is_extended_rep: bool) -> PolygonTexturing {
    let axis_u = read_vec3(reader);
    let axis_v = read_vec3(reader);

    let (u, v, texture_num, origin_vertex, cached_surface) = if is_extended_rep {
        (
            reader.read_f32::<byteorder::LittleEndian>().unwrap() * 4096.0,
            reader.read_f32::<byteorder::LittleEndian>().unwrap() * 4096.0,
            reader.read_u16::<byteorder::LittleEndian>().unwrap(),
            reader.read_u16::<byteorder::LittleEndian>().unwrap(),
            0,
        )
    } else {
        (
            f32::from(reader.read_u16::<byteorder::LittleEndian>().unwrap()),
            f32::from(reader.read_u16::<byteorder::LittleEndian>().unwrap()),
            reader.read_u8().unwrap() as u16,
            reader.read_u8().unwrap() as u16,
            reader.read_u16::<byteorder::LittleEndian>().unwrap(),
        )
    };

    let scale = reader.read_f32::<byteorder::LittleEndian>().unwrap();
    let center = read_vec3(reader);

    PolygonTexturing {
        axis_u,
        axis_v,
        u,
        v,
        texture_num,
        origin_vertex,
        cached_surface,
        scale,
        center,
    }
}

fn read_lights<T: io::Read>(
    packer: &mut TexturePacker<image::Rgb<u8>>,
    poly_idx: u32,
    reader: &mut T,
    num_lights: u8,
    num_lightmaps: u8,
    light_size: u8,
) -> (Vec<LightInfo>, Vec<u16>) {
    // Cell-local order for the animated-light bits on each face. A set bit in
    // `LightInfo::animation_flags` means the following layer belongs to the
    // light number at the same index in this table.
    let animated_light_numbers = (0..num_lights)
        .map(|_| reader.read_i16::<byteorder::LittleEndian>().unwrap())
        .collect::<Vec<_>>();

    let mut light_infos: Vec<LightInfo> = Vec::new();
    for _ in 0..num_lightmaps {
        let li = read_light_info(poly_idx, reader);
        light_infos.push(li);
    }

    for i in 0..num_lightmaps {
        let li = light_infos.get_mut(i as usize).unwrap();
        let lm_count = li.animation_flags.count_ones() + 1;

        let lm_size = (light_size as u16) * li.lx * (li.ly as u16);

        for idx in 0..lm_count {
            let mut bytes = vec![0_u8; lm_size as usize];
            reader.read_exact(&mut bytes).unwrap();

            let img = decode_lightmap(&bytes, li.lx, li.ly, light_size);

            if idx == 0 {
                li.texture_pack_result = packer.pack(&img);
                if lm_count > 1 {
                    li.base_pixels = Some(img.into_raw());
                }
            } else {
                let layer_number = idx - 1;
                let bit_index = set_bit_indices(li.animation_flags)
                    .nth(layer_number as usize)
                    .expect("animated lightmap layer must have a corresponding flag bit");
                let light_number = *animated_light_numbers
                    .get(bit_index)
                    .expect("animated lightmap flag must index the cell light table");
                li.switchable_layers.push(SwitchableLightmapLayer {
                    light_number,
                    pixels: img.into_raw(),
                });
            }
        }
    }

    // The cell's object-light list. The u32 counts the u16s that follow, and
    // the first of those is itself the number of real indices behind it.
    let light_index_count = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let mut light_indices = Vec::new();
    for i in 0..light_index_count {
        let index = reader.read_u16::<byteorder::LittleEndian>().unwrap();
        if i == 0 {
            debug_assert_eq!(
                u32::from(index) + 1,
                light_index_count,
                "cell {poly_idx}: leading light-list entry should count the rest"
            );
            continue;
        }
        light_indices.push(index);
    }

    (light_infos, light_indices)
}

fn set_bit_indices(flags: u32) -> impl Iterator<Item = usize> {
    (0..u32::BITS as usize).filter(move |bit| flags & (1 << bit) != 0)
}

fn decode_lightmap(bytes: &[u8], width: u16, height: u8, light_size: u8) -> image::RgbImage {
    image::ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
        let pixel_index = (y * width as u32 + x) as usize;
        match light_size {
            1 => {
                let value = bytes[pixel_index];
                image::Rgb([value, value, value])
            }
            _ => {
                let pos = pixel_index * light_size as usize;
                let pix = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let r = (pix & 0b0001_1111) << 3;
                let g = ((pix >> 5) & 0b0001_1111) << 3;
                let b = ((pix >> 10) & 0b0001_1111) << 3;
                image::Rgb([r as u8, g as u8, b as u8])
            }
        }
    })
}

#[derive(Debug, Clone)]
pub struct SwitchableLightmapLayer {
    pub light_number: i16,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LightInfo {
    pub debug_idx: u32,
    pub u: i16,
    pub v: i16,
    pub lx: u16,
    pub ly: u8,
    pub lx8: u8,
    pub static_lightmap_pointer: u32,
    pub dynamic_lightmap_pointer: u32,
    pub animation_flags: u32,
    pub texture_pack_result: TexturePackResult,
    /// Static pixels retained only for faces that have switchable layers.
    pub base_pixels: Option<Vec<u8>>,
    pub switchable_layers: Vec<SwitchableLightmapLayer>,
}

fn read_light_info<T: io::Read>(debug_idx: u32, reader: &mut T) -> LightInfo {
    let u = reader.read_i16::<byteorder::LittleEndian>().unwrap();
    let v = reader.read_i16::<byteorder::LittleEndian>().unwrap();

    let lx = reader.read_u16::<byteorder::LittleEndian>().unwrap();
    let ly = reader.read_u8().unwrap();
    let lx8 = reader.read_u8().unwrap();

    let static_lightmap_pointer = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let dynamic_lightmap_pointer = reader.read_u32::<byteorder::LittleEndian>().unwrap();
    let animation_flags = reader.read_u32::<byteorder::LittleEndian>().unwrap();

    LightInfo {
        debug_idx,
        u,
        v,
        lx,
        ly,
        lx8,
        static_lightmap_pointer,
        dynamic_lightmap_pointer,
        animation_flags,
        texture_pack_result: TexturePackResult::DEFAULT,
        base_pixels: None,
        switchable_layers: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_flags_follow_cell_light_number_order() {
        let flags = 0b1010;
        assert_eq!(set_bit_indices(flags).collect::<Vec<_>>(), vec![1, 3]);
        let cell_light_numbers = [12, 195, 27, 311];
        let mapped = set_bit_indices(flags)
            .map(|bit| cell_light_numbers[bit])
            .collect::<Vec<_>>();
        assert_eq!(mapped, vec![195, 311]);
    }

    #[test]
    fn rgb16_lightmaps_decode_to_rgb888() {
        let white = 0b0111_1111_1111_1111u16.to_le_bytes();
        let image = decode_lightmap(&white, 1, 1, 2);
        assert_eq!(image.get_pixel(0, 0).0, [248, 248, 248]);
    }
}
