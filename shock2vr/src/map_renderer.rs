use cgmath::{Quaternion, Vector3};
use dark::importers::TEXTURE_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{basic_material, color_material, SceneObject, UI2DRenderer},
};

/// Map rendering constants
const MAP_WIDTH: f32 = 614.0; // PAGE001.PCX dimensions
const MAP_HEIGHT: f32 = 260.0;

/// Helper function to load texture material with fallback
fn load_texture_material(
    asset_cache: &mut AssetCache,
    path: &str,
    fallback_color: Vector3<f32>,
) -> Box<dyn engine::scene::Material> {
    if let Some(texture) = asset_cache.get_opt(&TEXTURE_IMPORTER, path) {
        let texture_trait: std::rc::Rc<dyn engine::texture::TextureTrait> = texture;
        basic_material::create(texture_trait, 1.0, 0.0)
    } else {
        color_material::create(fallback_color)
    }
}

/// Reusable map renderer component that can be positioned anywhere in 3D space
pub struct MapRenderer {
    pub mission_name: String,
    pub map_data: Option<dark::map::MapChunkData>,
    pub revealed_slots: Vec<bool>,
    pub world_position: Vector3<f32>,
    pub world_rotation: Quaternion<f32>,
    pub scale: f32,
}

impl MapRenderer {
    pub fn new(mission_name: String, world_position: Vector3<f32>, scale: f32) -> Self {
        Self {
            mission_name,
            map_data: None,             // Will be loaded on first render
            revealed_slots: Vec::new(), // Will be initialized when map_data is loaded
            world_position,
            world_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scale,
        }
    }

    /// Initialize map data using asset cache (called automatically on first render)
    fn ensure_map_data_loaded(&mut self, asset_cache: &mut AssetCache) {
        if self.map_data.is_none() {
            if let Ok(map_data) =
                dark::map::MapChunkData::load_from_mission(asset_cache, &self.mission_name)
            {
                let slot_count = map_data.chunk_count();
                self.revealed_slots = vec![false; slot_count];
                self.map_data = Some(map_data);
            }
        }
    }

    pub fn set_revealed_slots(&mut self, slots: &[usize]) {
        // Reset all slots
        for slot in &mut self.revealed_slots {
            *slot = false;
        }

        // Reveal specified slots
        for &slot_idx in slots {
            if slot_idx < self.revealed_slots.len() {
                self.revealed_slots[slot_idx] = true;
            }
        }
    }

    pub fn get_revealed_slot_count(&self) -> usize {
        self.revealed_slots
            .iter()
            .filter(|&&revealed| revealed)
            .count()
    }

    pub fn render(&mut self, asset_cache: &mut AssetCache) -> Vec<SceneObject> {
        // Ensure map data is loaded
        self.ensure_map_data_loaded(asset_cache);
        // Create 2D UI renderer with proper coordinate system
        let mut ui = UI2DRenderer::new_with_flips(
            self.world_position,
            (MAP_WIDTH, MAP_HEIGHT),
            self.scale,
            true, // Flip X to match original working behavior
            true, // Flip Y to correct upside-down PCX
            self.world_rotation,
        );

        // Add background
        let background_texture_path =
            format!("{}/english/PAGE001.PCX", self.mission_name.to_uppercase());
        let background_material = load_texture_material(
            asset_cache,
            &background_texture_path,
            cgmath::vec3(0.3, 0.3, 0.8),
        );
        ui.add_rect(background_material, 0.0, 0.0, MAP_WIDTH, MAP_HEIGHT, 0.02);

        // Add revealed chunks
        if let Some(ref map_data) = self.map_data {
            for (slot_idx, &is_revealed) in self.revealed_slots.iter().enumerate() {
                if !is_revealed {
                    continue;
                }

                if let Some(rect) = map_data.get_revealed_rect(slot_idx) {
                    let chunk_texture_path = format!(
                        "{}/english/P001R{:03}.PCX",
                        self.mission_name.to_uppercase(),
                        slot_idx
                    );
                    let chunk_material = load_texture_material(
                        asset_cache,
                        &chunk_texture_path,
                        cgmath::vec3(1.0, 1.0, 0.0),
                    );
                    ui.add_rect(
                        chunk_material,
                        rect.ul_x as f32,
                        rect.ul_y as f32,
                        rect.width() as f32,
                        rect.height() as f32,
                        -0.005,
                    );
                }
            }
        }

        ui.render_objects()
    }
}
