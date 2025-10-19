use std::{cell::RefCell, collections::HashMap, env, rc::Rc, time::Duration};

use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPositionTextureLightmapAtlasNormal, VertexPositionTextureNormal},
    texture::{AnimatedTexture, Texture, TextureTrait},
};

use crate::{
    importers::TEXTURE_IMPORTER, properties::RenderType, util::load_multiple_textures_for_family,
};

pub fn to_scene(
    level: &crate::mission::SystemShock2Level,
    asset_cache: &mut AssetCache,
) -> Vec<SceneObject> {
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
                ))
            }
        };

        let scene_object1 = engine::scene::scene_object::SceneObject::create(material, mesh);
        scene_objects.push(scene_object1)
    }

    scene_objects
}
