use std::collections::HashMap;

use cgmath::{Deg, Matrix4, Quaternion, Rotation3, Vector2, Vector3, vec3};
use dark::{
    SCALE_FACTOR,
    mission::{SongParams, room_database::RoomDatabase},
    ss2_entity_info::SystemShock2EntityInfo,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{SceneObject, color_material, light::SpotLight},
};
use rapier3d::prelude::{Collider, ColliderBuilder};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        AbstractMission, AlwaysVisible, GlobalContext, SpawnLocation,
        entity_populator::empty_entity_populator::EmptyEntityPopulator, mission_core::MissionCore,
    },
    quest_info::QuestInfo,
    save_load::HeldItemSaveData,
    scripts::{Effect, GlobalEffect},
    time::Time,
};

/// Convenience builder for MissionCore-backed debug scenes that only need a floor and spawn point.
pub struct DebugSceneBuilder {
    scene_name: String,
    spawn_location: SpawnLocation,
    floor: Option<DebugSceneFloor>,
    extra_scene_objects: Vec<SceneObject>,
    physics_geometry: Option<Collider>,
}

impl DebugSceneBuilder {
    pub fn new(scene_name: impl Into<String>) -> Self {
        Self {
            scene_name: scene_name.into(),
            spawn_location: SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0 / SCALE_FACTOR),
                Quaternion::from_angle_y(Deg(90.0)),
            ),
            floor: None,
            extra_scene_objects: Vec::new(),
            physics_geometry: None,
        }
    }

    pub fn with_spawn_location(mut self, spawn_location: SpawnLocation) -> Self {
        self.spawn_location = spawn_location;
        self
    }

    pub fn with_floor(mut self, floor: DebugSceneFloor) -> Self {
        self.floor = Some(floor);
        self
    }

    pub fn add_scene_object(mut self, scene_object: SceneObject) -> Self {
        self.extra_scene_objects.push(scene_object);
        self
    }

    pub fn with_physics_geometry(mut self, collider: Collider) -> Self {
        self.physics_geometry = Some(collider);
        self
    }

    /// Add a standard floor suitable for most debug scenes (120x120 units, dark blue-gray)
    pub fn with_default_floor(self) -> Self {
        const DEFAULT_FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);
        const DEFAULT_FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);

        self.with_floor(DebugSceneFloor::ss2_units(
            DEFAULT_FLOOR_SIZE,
            DEFAULT_FLOOR_COLOR,
        ))
    }

    /// Reset to the default spawn location (5 units above origin, facing east)
    pub fn with_default_spawn_location(self) -> Self {
        self.with_spawn_location(SpawnLocation::PositionRotation(
            vec3(0.0, 5.0 / SCALE_FACTOR, 0.0 / SCALE_FACTOR),
            Quaternion::from_angle_y(Deg(90.0)),
        ))
    }

    pub fn build(self, options: DebugSceneBuildOptions<'_>) -> DebugScene {
        DebugScene {
            core: self.build_core(options),
        }
    }

    pub fn build_with_hooks<H>(
        self,
        options: DebugSceneBuildOptions<'_>,
        hooks: H,
    ) -> Box<dyn GameScene>
    where
        H: DebugSceneHooks + 'static,
    {
        let core = self.build_core(options);
        Box::new(HookedDebugScene::new(core, hooks))
    }

    pub fn build_core(self, options: DebugSceneBuildOptions<'_>) -> MissionCore {
        let mut scene_objects = Vec::new();
        let mut physics_geometry = self.physics_geometry;

        if let Some(floor) = self.floor {
            let (mut floor_objects, floor_collider) = floor.build();
            scene_objects.append(&mut floor_objects);
            if physics_geometry.is_none() {
                physics_geometry = Some(floor_collider);
            }
        }

        scene_objects.extend(self.extra_scene_objects);

        let abstract_mission = AbstractMission {
            scene_objects,
            song_params: SongParams {
                song: String::new(),
            },
            room_db: RoomDatabase { rooms: Vec::new() },
            physics_geometry,
            spatial_data: None,
            entity_info: SystemShock2EntityInfo::empty(),
            obj_map: HashMap::new(),
            visibility_engine: Box::new(AlwaysVisible),
        };

        MissionCore::load(
            self.scene_name,
            abstract_mission,
            options.asset_cache,
            options.audio_context,
            options.global_context,
            self.spawn_location,
            QuestInfo::new(),
            Box::new(EmptyEntityPopulator {}),
            HeldItemSaveData::empty(),
            options.game_options,
        )
    }
}

pub struct DebugSceneBuildOptions<'a> {
    pub global_context: &'a GlobalContext,
    pub game_options: &'a GameOptions,
    pub asset_cache: &'a mut AssetCache,
    pub audio_context: &'a mut AudioContext<EntityId, String>,
}

/// Wrapper that already implements GameScene by delegating to MissionCore.
pub struct DebugScene {
    core: MissionCore,
}

impl DebugScene {
    pub fn core(&self) -> &MissionCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut MissionCore {
        &mut self.core
    }
}

impl GameScene for DebugScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.core.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.core.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.core
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.core
            .finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.core.queue_entity_trigger(entity_name)
    }
}

pub trait DebugSceneHooks {
    #[allow(clippy::too_many_arguments)]
    fn before_update(
        &mut self,
        _core: &mut MissionCore,
        _time: &Time,
        _input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
    }

    #[allow(clippy::too_many_arguments)]
    fn before_handle_effects(
        &mut self,
        _core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) {
    }

    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        _scene_objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) {
    }
}

pub struct HookedDebugScene<H> {
    core: MissionCore,
    hooks: H,
}

impl<H> HookedDebugScene<H> {
    pub fn new(core: MissionCore, hooks: H) -> Self {
        Self { core, hooks }
    }
}

impl<H: DebugSceneHooks> GameScene for HookedDebugScene<H> {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.hooks.before_update(
            &mut self.core,
            time,
            input_context,
            asset_cache,
            game_options,
        );
        self.core.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let (mut scene_objects, mut camera_position, mut camera_rotation) =
            self.core.render(asset_cache, options);
        self.hooks.after_render(
            &mut self.core,
            &mut scene_objects,
            &mut camera_position,
            &mut camera_rotation,
            asset_cache,
            options,
        );
        (scene_objects, camera_position, camera_rotation)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.core
            .render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.core
            .finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        let mut effects = effects;
        self.hooks.before_handle_effects(
            &mut self.core,
            &mut effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );
        self.core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.core.get_hand_spotlights(options)
    }

    fn world(&self) -> &shipyard::World {
        self.core.world()
    }

    fn scene_name(&self) -> &str {
        self.core.scene_name()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.core.queue_entity_trigger(entity_name)
    }
}

#[derive(Clone, Copy)]
pub enum DebugSceneFloorUnits {
    /// Provided size is in SS2 units and should be divided by SCALE_FACTOR.
    SystemShock2,
    /// Provided size is already in world meters.
    World,
}

pub struct DebugSceneFloor {
    pub size: Vector3<f32>,
    pub color: Vector3<f32>,
    pub units: DebugSceneFloorUnits,
}

impl DebugSceneFloor {
    pub fn ss2_units(size: Vector3<f32>, color: Vector3<f32>) -> Self {
        Self {
            size,
            color,
            units: DebugSceneFloorUnits::SystemShock2,
        }
    }

    pub fn world_units(size: Vector3<f32>, color: Vector3<f32>) -> Self {
        Self {
            size,
            color,
            units: DebugSceneFloorUnits::World,
        }
    }

    fn build(self) -> (Vec<SceneObject>, Collider) {
        let (visual_scale, collider_scale) = match self.units {
            DebugSceneFloorUnits::SystemShock2 => (1.0 / SCALE_FACTOR, 1.0 / SCALE_FACTOR),
            DebugSceneFloorUnits::World => (1.0, 1.0),
        };

        let visual_size = self.size * visual_scale;
        let collider_half_size = (self.size * collider_scale) * 0.5;

        let floor_transform = Matrix4::from_translation(vec3(0.0, 0.0, 0.0))
            * Matrix4::from_nonuniform_scale(visual_size.x, visual_size.y, visual_size.z);

        let floor_material = color_material::create(self.color);
        let mut floor_object =
            SceneObject::new(floor_material, Box::new(engine::scene::cube::create()));
        floor_object.set_transform(floor_transform);

        let collider = ColliderBuilder::cuboid(
            collider_half_size.x,
            collider_half_size.y,
            collider_half_size.z,
        )
        .build();

        (vec![floor_object], collider)
    }
}
