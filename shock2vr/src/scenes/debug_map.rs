use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector3, point2, vec3};
use dark::properties::{PropMapRef, PropPosition};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, light::SpotLight},
};
use shipyard::{EntityId, UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    gui::{GUI_PIXEL_TO_WORLD_SIZE, Gui},
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo, PlayerMapLocation},
    quest_info::QuestInfo,
    runtime_props::RuntimePropMapData,
    scripts::Effect,
    scripts::gui::{MapGui, MapGuiState},
    time::Time,
};

/// Debug scene constants. The mission string doubles as the `QuestInfo`
/// explored-map key and (via its stem) the per-level art directory.
const MAP_MISSION: &str = "medsci1.mis";
const LOCATION_REVEAL_INTERVAL: f32 = 1.0; // Reveal one map location every second
/// Shrink the panel's native world size (`pixels * GUI_PIXEL_TO_WORLD_SIZE`,
/// ~2.5m wide) to comfortably fit the debug camera's view.
const PANEL_WORLD_SCALE: f32 = 0.5;
/// Nudge each successive panel component toward the camera so coplanar decals
/// draw over the page art (a debug-presentation offset; the shared composition
/// emits coplanar components and relies on draw order elsewhere).
const COMPONENT_Z_STEP: f32 = 0.002;

/// The real medsci1 `MapRef` markers (mission ids 1032/1034 + the two inset
/// relocation markers; the same values verified by `scripts/gui/map.rs` tests):
/// (frame, world (x, z), page (x, y)). The two `frame == -1` entries are the
/// global scale references; frames 0 and 2 relocate the "INSET LOWER LEVEL"
/// boxes, so cycling the current location exercises per-frame pip relocation.
const MEDSCI1_MAP_REF_MARKERS: [(i32, (f32, f32), (i32, i32)); 4] = [
    (-1, (-40.311_17, 32.874_435), (536, 239)),
    (-1, (44.685_417, -76.398_605), (232, 10)),
    (0, (11.585_943, 7.603_475), (92, 30)),
    (2, (-19.003_445, -54.242_805), (72, 127)),
];

/// Debug scene for the automap composition: drives the in-game panel's
/// `MapGui` (page art, dim explored vs bright current decals, `MapRef` player
/// pip) and renders its components as a world-space quad stack - the same
/// composition path as the flat automap panel, so the two cannot drift.
pub struct DebugMapScene {
    world: World,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_rotation: Quaternion<f32>,
    scene_name: String,
    /// The synthetic panel entity carrying `RuntimePropMapData` (attached on
    /// the first update, once the asset cache is available).
    map_entity: EntityId,
    map_data_loaded: bool,
    location_count: usize,
    next_location: i32,
    reveal_timer: f32,
}

impl DebugMapScene {
    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    fn head_base(&self) -> Vector3<f32> {
        self.player_position + vec3(0.0, 1.5, 0.0) // 1.5m head height
    }

    pub fn new() -> Self {
        let mut world = World::new();

        let player_entity = world.add_entity(());
        let inventory_entity = PlayerInventoryEntity::create(&mut world);
        PlayerInventoryEntity::set_position_rotation(
            &mut world,
            vec3(0.0, -1000.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );

        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });
        world.add_unique(GlobalEntityMetadata(HashMap::new()));
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(QuestInfo::new());
        world.add_unique(PlayerMapLocation::default());
        world.add_unique(Time::default());

        // The panel entity `MapGui` composes from; its page data is attached
        // lazily (loading the rects needs the asset cache).
        let map_entity = world.add_entity(());

        // MapRef marker entities, exactly as a mission authors them, so the
        // panel's player-pip placement runs against real data.
        for (frame, world_pos, page) in MEDSCI1_MAP_REF_MARKERS {
            world.add_entity((
                PropMapRef {
                    x: page.0,
                    y: page.1,
                    frame,
                },
                PropPosition {
                    position: vec3(world_pos.0, 0.0, world_pos.1),
                    rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    cell: 0,
                },
            ));
        }

        Self {
            world,
            player_position: vec3(0.0, 0.0, 0.0),
            player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scene_name: "Debug Map".to_string(),
            map_entity,
            map_data_loaded: false,
            location_count: 0,
            next_location: 0,
            reveal_timer: 0.0,
        }
    }
}

impl GameScene for DebugMapScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = command_effects;

        // Update world time
        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        // Update player head position from input
        self.head_rotation = input_context.head.rotation;
        self.update_player_info();

        // Attach the level's page data to the panel entity on first update -
        // the same `RuntimePropMapData` the mission attaches at init. Like
        // `mission_core`, a failed load attaches empty rect lists, which
        // `MapGui` renders as the original's `nomap` art.
        if !self.map_data_loaded {
            self.map_data_loaded = true;
            let level = MAP_MISSION.split('.').next().unwrap_or(MAP_MISSION);
            let (revealed_rects, explored_rects) =
                dark::map::MapChunkData::load_from_mission(asset_cache, level)
                    .map(|data| (data.revealed_rects, data.explored_rects))
                    .unwrap_or_default();
            self.location_count = revealed_rects.len().max(explored_rects.len());
            self.world.add_component(
                self.map_entity,
                RuntimePropMapData {
                    mission: MAP_MISSION.to_string(),
                    revealed_rects,
                    explored_rects,
                },
            );
        }

        // Walk the map every LOCATION_REVEAL_INTERVAL seconds: reveal the next
        // location and make it the player's current one, so the panel shows
        // its bright art (and the dim explored art behind it) plus the pip.
        self.reveal_timer += time.elapsed.as_secs_f32();
        if self.reveal_timer >= LOCATION_REVEAL_INTERVAL && self.location_count > 0 {
            self.reveal_timer = 0.0;
            let location = self.next_location;
            if let Ok(mut quest_info) = self.world.borrow::<UniqueViewMut<QuestInfo>>() {
                quest_info.reveal_map_location(MAP_MISSION, location);
            }
            if let Ok(mut current) = self.world.borrow::<UniqueViewMut<PlayerMapLocation>>() {
                current.0 = Some(location);
            }
            self.next_location = (location + 1) % self.location_count as i32;
        }

        Vec::new()
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Position the panel in front of the player's head, facing the camera.
        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        let head_position = self.head_base();
        let map_world_position = head_position + forward * 1.5; // 1.5 meters in front

        let mut look_dir = head_position - map_world_position;
        if look_dir.magnitude2() < 1e-6 {
            look_dir = vec3(0.0, 0.0, 1.0);
        } else {
            look_dir = look_dir.normalize();
        }

        let mut up = vec3(0.0, 1.0, 0.0);
        let mut right = look_dir.cross(up);
        if right.magnitude2() < 1e-6 {
            up = vec3(0.0, 0.0, 1.0);
            right = look_dir.cross(up);
        }
        right = right.normalize();
        let true_up = right.cross(look_dir).normalize();
        let rotation_matrix = Matrix3::from_cols(right, true_up, -look_dir);

        // Compose the panel through MapGui - the automap's composition path -
        // and present its components exactly like the VR world-quad path
        // (`GuiComponent::to_render_info` + `GuiComponentRenderInfo::render`,
        // as `GuiManager` does), anchored at the debug panel transform.
        let map_gui = MapGui;
        let components =
            map_gui.get_components(&None, self.map_entity, &self.world, &MapGuiState::default());
        let screen_size = map_gui.get_config().screen_size_in_pixels;
        let world_size = screen_size * GUI_PIXEL_TO_WORLD_SIZE * PANEL_WORLD_SCALE;
        let root_transform = Matrix4::from_translation(map_world_position)
            * Matrix4::from(Quaternion::from(rotation_matrix))
            * Matrix4::from_nonuniform_scale(world_size.x, world_size.y, 1.0);

        let objects = components
            .into_iter()
            .enumerate()
            .map(|(index, component)| {
                // Draw opaque, like the flat host does ("the original MFD art
                // is opaque on screen") - the shared components' default alpha
                // is a VR world-quad translucency. No cursor over the debug
                // panel (MapGui has no buttons).
                let info = component
                    .with_alpha(1.0)
                    .to_render_info(screen_size, point2(-1.0, -1.0));
                let mut object = info.render(asset_cache);
                object.set_transform(
                    root_transform
                        * Matrix4::from_translation(vec3(
                            0.0,
                            0.0,
                            -COMPONENT_Z_STEP * index as f32,
                        )),
                );
                object
            })
            .collect();

        (objects, self.player_position, self.player_rotation)
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }
}
