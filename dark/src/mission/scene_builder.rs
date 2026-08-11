use std::{cell::RefCell, collections::HashMap, env, rc::Rc, time::Duration};

use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPositionTextureLightmapAtlasNormal, VertexPositionTextureNormal},
    texture::{AnimatedTexture, Texture, TextureTrait},
};

use crate::{
    importers::TEXTURE_IMPORTER, properties::RenderType, util::load_multiple_textures_for_family,
};

/// GPU scene plus the sparse controller for authored switchable lightmaps.
pub struct MissionScene {
    pub objects: Vec<SceneObject>,
    pub animated_lightmaps: AnimatedLightmapController,
}

struct AnimatedLightmapRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    base_pixels: Vec<u8>,
    layers: Vec<super::SwitchableLightmapLayer>,
    dirty: bool,
}

/// Rare state changes update only the affected atlas rectangles. The ordinary
/// frame shader remains the same single lightmap sample on desktop and Quest.
pub struct AnimatedLightmapController {
    texture: Rc<Texture>,
    regions: Vec<AnimatedLightmapRegion>,
    light_to_regions: HashMap<i16, Vec<usize>>,
    intensities: HashMap<i16, f32>,
    compose_scratch: Vec<u8>,
}

impl AnimatedLightmapController {
    fn new(level: &crate::mission::SystemShock2Level, texture: Rc<Texture>) -> Self {
        let mut regions = Vec::new();
        let mut light_to_regions: HashMap<i16, Vec<usize>> = HashMap::new();

        for light_info in level.cells.iter().flat_map(|cell| &cell.lights) {
            let Some(base_pixels) = &light_info.base_pixels else {
                continue;
            };
            if light_info.switchable_layers.is_empty() {
                continue;
            }

            let placement = light_info.texture_pack_result;
            let region_index = regions.len();
            for layer in &light_info.switchable_layers {
                light_to_regions
                    .entry(layer.light_number)
                    .or_default()
                    .push(region_index);
            }
            regions.push(AnimatedLightmapRegion {
                x: (placement.uv_offset_x * texture.width() as f32).round() as u32,
                y: (placement.uv_offset_y * texture.height() as f32).round() as u32,
                width: light_info.lx as u32,
                height: light_info.ly as u32,
                base_pixels: base_pixels.clone(),
                layers: light_info.switchable_layers.clone(),
                dirty: false,
            });
        }

        tracing::debug!(
            lights = light_to_regions.len(),
            regions = regions.len(),
            "prepared switchable lightmaps"
        );
        Self {
            texture,
            regions,
            light_to_regions,
            intensities: HashMap::new(),
            compose_scratch: Vec::new(),
        }
    }

    /// Queue one authored light value. Repeated effects in a script batch mark
    /// rectangles only; [`flush`](Self::flush) recomposes each at most once.
    pub fn set_light_intensity(&mut self, light_number: i16, intensity: f32) -> bool {
        let Some(affected_regions) = self.light_to_regions.get(&light_number) else {
            return false;
        };
        let intensity = intensity.clamp(0.0, 1.0);
        if self.intensities.get(&light_number).copied() == Some(intensity) {
            return true;
        }

        self.intensities.insert(light_number, intensity);
        for &region_index in affected_regions {
            self.regions[region_index].dirty = true;
        }
        true
    }

    /// Upload every dirty rectangle. Cost scales with the authored affected
    /// pixels, not the 4096² atlas or the total entity count.
    pub fn flush(&mut self) {
        for region in &mut self.regions {
            if !region.dirty {
                continue;
            }
            compose_region(
                &region.base_pixels,
                &region.layers,
                &self.intensities,
                &mut self.compose_scratch,
            );
            self.texture.update_rgb_region(
                region.x,
                region.y,
                region.width,
                region.height,
                &self.compose_scratch,
            );
            region.dirty = false;
        }
    }

    pub fn light_count(&self) -> usize {
        self.light_to_regions.len()
    }

    pub fn region_count(&self) -> usize {
        self.regions.len()
    }
}

fn compose_region(
    base_pixels: &[u8],
    layers: &[super::SwitchableLightmapLayer],
    intensities: &HashMap<i16, f32>,
    output: &mut Vec<u8>,
) {
    output.clear();
    output.extend_from_slice(base_pixels);
    for layer in layers {
        let intensity = intensities
            .get(&layer.light_number)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        if intensity <= 0.0 {
            continue;
        }
        for (target, contribution) in output.iter_mut().zip(&layer.pixels) {
            *target = (*target as f32 + *contribution as f32 * intensity)
                .min(255.0)
                .round() as u8;
        }
    }
}

pub fn to_scene(
    level: &crate::mission::SystemShock2Level,
    asset_cache: &mut AssetCache,
) -> MissionScene {
    let lightmap_textures = level.lightmap_atlas.generate_textures();
    let tex = lightmap_textures.get(0).unwrap();
    let lightmap_texture = tex.clone();
    let debug_normals_enabled = env::var_os("SS2_DEBUG_NORMALS").is_some();

    let all_geometry = &level.all_geometry;
    let mut texture_to_vertices: HashMap<&u16, Vec<VertexPositionTextureLightmapAtlasNormal>> =
        HashMap::new();
    for geometry in all_geometry {
        let texture_id = &geometry.texture_idx;

        // Skip empty texture
        if *texture_id == 0 {
            continue;
        }

        // Ensure we have an entry
        texture_to_vertices
            .entry(texture_id)
            .or_insert_with(Vec::new);

        let current_vertices = texture_to_vertices.get_mut(texture_id).unwrap();

        let mut verts = geometry.verts.clone();
        current_vertices.append(&mut verts);
    }

    let mut scene_objects = Vec::new();
    for (texture_id, vertices) in texture_to_vertices {
        if debug_normals_enabled {
            let simple_vertices: Vec<VertexPositionTextureNormal> = vertices
                .into_iter()
                .map(|v| VertexPositionTextureNormal {
                    position: v.position,
                    uv: v.uv,
                    normal: v.normal,
                })
                .collect();

            let geometry: Rc<Box<dyn engine::scene::Geometry>> =
                Rc::new(Box::new(engine::scene::mesh::create(simple_vertices)));

            let material = RefCell::new(engine::scene::debug_normal_material::create());

            let scene_object = engine::scene::scene_object::SceneObject::create(material, geometry);
            scene_objects.push(scene_object);
            continue;
        }

        let tex_info = &level.textures.0[*texture_id as usize];
        let initial_texture: &Rc<Texture> = {
            &asset_cache
                .get(
                    &TEXTURE_IMPORTER,
                    &format!(
                        "{}/{}.PCX",
                        tex_info.family.to_uppercase(),
                        tex_info.texture_filename
                    ),
                )
                .clone()
        };

        let animated_texture: Rc<dyn TextureTrait> =
            if let Some(animation_info) = &tex_info.animation_info {
                let mut additional_textures = load_multiple_textures_for_family(
                    asset_cache,
                    &tex_info.family,
                    &tex_info.texture_filename,
                );
                additional_textures.insert(0, initial_texture.clone());
                Rc::new(AnimatedTexture::new(
                    additional_textures,
                    Duration::from_millis(animation_info.rate_in_milliseconds as u64),
                ))
            } else {
                initial_texture.clone()
            };

        let mesh: Rc<Box<dyn engine::scene::Geometry>> =
            Rc::new(Box::new(engine::scene::mesh::create(vertices)));

        let material = {
            if tex_info.render_type == RenderType::FullBright {
                RefCell::new(engine::scene::basic_material::create(
                    animated_texture,
                    1.0,
                    0.0,
                ))
            } else {
                RefCell::new(engine::materials::LightmapMaterial::create(
                    lightmap_texture.clone(),
                    animated_texture,
                    level.render_params.ambient_color,
                ))
            }
        };

        let scene_object1 = engine::scene::scene_object::SceneObject::create(material, mesh);
        scene_objects.push(scene_object1)
    }

    MissionScene {
        objects: scene_objects,
        animated_lightmaps: AnimatedLightmapController::new(level, lightmap_texture),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::SwitchableLightmapLayer;

    #[test]
    fn switchable_layers_add_to_static_light_and_saturate() {
        let layers = vec![
            SwitchableLightmapLayer {
                light_number: 195,
                pixels: vec![100, 20, 0],
            },
            SwitchableLightmapLayer {
                light_number: 311,
                pixels: vec![80, 80, 80],
            },
        ];
        let intensities = HashMap::from([(195, 1.0), (311, 0.5)]);
        let mut output = Vec::new();

        compose_region(&[140, 200, 250], &layers, &intensities, &mut output);

        assert_eq!(output, vec![255, 255, 255]);
    }

    #[test]
    fn absent_light_intensity_leaves_static_pixels_unchanged() {
        let layers = vec![SwitchableLightmapLayer {
            light_number: 195,
            pixels: vec![100, 100, 100],
        }];
        let mut output = Vec::new();

        compose_region(&[10, 20, 30], &layers, &HashMap::new(), &mut output);

        assert_eq!(output, vec![10, 20, 30]);
    }
}
